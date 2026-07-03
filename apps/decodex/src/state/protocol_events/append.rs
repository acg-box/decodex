use crate::{
	prelude::{Result, eyre},
	state::{
		StateData, StateStore,
		protocol_events::{archive, hash},
		runtime_records::ProtocolEventRecord,
		runtime_row_parsers,
	},
};

impl StateStore {
	/// Append one protocol event to the journal for a run.
	pub fn append_event(
		&self,
		run_id: &str,
		sequence_number: i64,
		event_type: &str,
		payload: &str,
	) -> Result<()> {
		let now = runtime_row_parsers::timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let event = ProtocolEventRecord {
			sequence_number,
			event_type: event_type.to_owned(),
			payload_sha256: hash::protocol_event_payload_sha256(payload),
			created_at: now.text,
			created_at_unix: now.unix,
		};
		let Some(mut event) =
			self.prepare_protocol_event_for_append_locked(&mut state, run_id, event)?
		else {
			return Ok(());
		};

		loop {
			let (insert_index, cached_existing) = {
				let events = state.events.entry(run_id.to_owned()).or_default();

				match events
					.binary_search_by_key(&event.sequence_number, |event| event.sequence_number)
				{
					Ok(index) => (index, Some(events[index].clone())),
					Err(index) => (index, None),
				}
			};

			if let Some(existing) = cached_existing {
				if self.handle_protocol_event_append_conflict_locked(
					&mut state, run_id, &mut event, &existing,
				)? {
					continue;
				}

				return Ok(());
			}

			if !self.append_protocol_event_locked(run_id, &event)? {
				let existing =
					self.protocol_event_locked(run_id, event.sequence_number)?.ok_or_else(|| {
						eyre::eyre!(
							"Protocol event `{run_id}` sequence `{}` already exists in the runtime journal, but the existing row could not be read.",
							event.sequence_number
						)
					})?;

				if self.handle_protocol_event_append_conflict_locked(
					&mut state, run_id, &mut event, &existing,
				)? {
					continue;
				}

				return Ok(());
			}

			self.record_inserted_protocol_event_locked(&mut state, run_id, insert_index, event)?;

			return Ok(());
		}
	}

	fn prepare_protocol_event_for_append_locked(
		&self,
		state: &mut StateData,
		run_id: &str,
		event: ProtocolEventRecord,
	) -> Result<Option<ProtocolEventRecord>> {
		if !self.protocol_event_should_be_discarded_after_archive_locked(state, run_id, &event)? {
			return Ok(Some(event));
		}
		if self.protocol_event_replay_already_recorded_locked(state, run_id, &event)? {
			self.refresh_protocol_event_summaries_for_runs_locked(state, &[run_id.to_owned()])?;

			return Ok(None);
		}

		Ok(Some(archive::discarded_post_archive_protocol_event_with_log(run_id, event)))
	}

	fn handle_protocol_event_append_conflict_locked(
		&self,
		state: &mut StateData,
		run_id: &str,
		event: &mut ProtocolEventRecord,
		existing: &ProtocolEventRecord,
	) -> Result<bool> {
		if archive::protocol_event_conflict_should_be_discarded_after_archive(existing, event) {
			*event = archive::discarded_post_archive_protocol_event_with_log(run_id, event.clone());

			return Ok(true);
		}
		if archive::protocol_event_is_discarded_post_archive_collision(existing, event) {
			event.sequence_number = archive::next_discarded_post_archive_sequence_after_collision(
				event.sequence_number,
			)?;

			return Ok(true);
		}

		archive::ensure_protocol_event_replay_matches(run_id, existing, event)?;

		self.refresh_protocol_event_summaries_for_runs_locked(state, &[run_id.to_owned()])?;

		Ok(false)
	}

	fn record_inserted_protocol_event_locked(
		&self,
		state: &mut StateData,
		run_id: &str,
		insert_index: usize,
		event: ProtocolEventRecord,
	) -> Result<()> {
		let had_cached_summary = state.event_summaries.contains_key(run_id);
		let inserted_event = event.clone();

		state.events.entry(run_id.to_owned()).or_default().insert(insert_index, event);

		if self.sqlite.is_some() && !had_cached_summary {
			self.rebuild_protocol_event_summaries_for_runs_locked(state, &[run_id.to_owned()])?;
		} else if self.sqlite.is_some() {
			let summary = state.event_summaries.entry(run_id.to_owned()).or_default();

			if summary.last_sequence_number.is_none_or(|last_sequence_number| {
				inserted_event.sequence_number == last_sequence_number.saturating_add(1)
			}) {
				summary.record_event(&inserted_event);

				let summary = summary.clone();

				self.upsert_protocol_event_summary_locked(run_id, &summary)?;
			} else {
				self.rebuild_protocol_event_summaries_for_runs_locked(state, &[run_id.to_owned()])?;
			}
		} else if let Some(events) = state.events.get(run_id) {
			let summary = runtime_row_parsers::protocol_event_summary_from_events(events);

			state.event_summaries.insert(run_id.to_owned(), summary);
		}

		Ok(())
	}

	fn protocol_event_should_be_discarded_after_archive_locked(
		&self,
		state: &StateData,
		run_id: &str,
		event: &ProtocolEventRecord,
	) -> Result<bool> {
		if !archive::protocol_event_can_be_discarded_after_archive(event) {
			return Ok(false);
		}

		self.run_has_terminal_thread_archive_event_locked(state, run_id)
	}

	fn protocol_event_replay_already_recorded_locked(
		&self,
		state: &StateData,
		run_id: &str,
		event: &ProtocolEventRecord,
	) -> Result<bool> {
		if let Some(events) = state.events.get(run_id)
			&& let Ok(index) =
				events.binary_search_by_key(&event.sequence_number, |event| event.sequence_number)
		{
			return Ok(events[index].is_idempotent_replay_of(event));
		}

		let Some(existing) = self.protocol_event_locked(run_id, event.sequence_number)? else {
			return Ok(false);
		};

		Ok(existing.is_idempotent_replay_of(event))
	}
}
