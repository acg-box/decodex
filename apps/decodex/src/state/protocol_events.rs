use sha2::{Digest as _, Sha256};

use super::{StateData, StateStore};
use crate::{
	prelude::{Result, eyre},
	state::{
		runtime_records::{ProtocolEventRecord, ProtocolEventSummaryRecord},
		runtime_row_parsers::{protocol_event_summary_from_events, timestamp_parts},
	},
};

const TERMINAL_THREAD_ARCHIVE_EVENT_TYPES: [&str; 2] =
	["thread/archive", "thread/archive/discarded"];
const DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE: &str = "protocol/post_archive_event/discarded";

impl StateStore {
	/// Return whether one run already has a matching protocol event.
	pub fn run_has_protocol_event(&self, run_id: &str, event_type: &str) -> Result<bool> {
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite.run_has_protocol_event(run_id, event_type);
		}

		let state = self.lock()?;

		Ok(state
			.events
			.get(run_id)
			.is_some_and(|events| events.iter().any(|event| event.event_type == event_type)))
	}

	/// Append one protocol event to the journal for a run.
	pub fn append_event(
		&self,
		run_id: &str,
		sequence_number: i64,
		event_type: &str,
		payload: &str,
	) -> Result<()> {
		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let event = ProtocolEventRecord {
			sequence_number,
			event_type: event_type.to_owned(),
			payload_sha256: protocol_event_payload_sha256(payload),
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

		Ok(Some(discarded_post_archive_protocol_event_with_log(run_id, event)))
	}

	fn handle_protocol_event_append_conflict_locked(
		&self,
		state: &mut StateData,
		run_id: &str,
		event: &mut ProtocolEventRecord,
		existing: &ProtocolEventRecord,
	) -> Result<bool> {
		if protocol_event_conflict_should_be_discarded_after_archive(existing, event) {
			*event = discarded_post_archive_protocol_event_with_log(run_id, event.clone());

			return Ok(true);
		}
		if protocol_event_is_discarded_post_archive_collision(existing, event) {
			event.sequence_number =
				next_discarded_post_archive_sequence_after_collision(event.sequence_number)?;

			return Ok(true);
		}

		ensure_protocol_event_replay_matches(run_id, existing, event)?;

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
			let summary = protocol_event_summary_from_events(events);

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
		if !protocol_event_can_be_discarded_after_archive(event) {
			return Ok(false);
		}

		self.run_has_terminal_thread_archive_event_locked(state, run_id)
	}

	fn run_has_terminal_thread_archive_event_locked(
		&self,
		state: &StateData,
		run_id: &str,
	) -> Result<bool> {
		if state.events.get(run_id).is_some_and(|events| {
			events.iter().any(|event| protocol_event_is_terminal_thread_archive(&event.event_type))
		}) {
			return Ok(true);
		}
		if state.event_summaries.get(run_id).is_some_and(|summary| {
			summary
				.last_event_type
				.as_deref()
				.is_some_and(protocol_event_is_terminal_thread_archive)
		}) {
			return Ok(true);
		}

		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(false);
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		for event_type in TERMINAL_THREAD_ARCHIVE_EVENT_TYPES {
			if sqlite.run_has_protocol_event(run_id, event_type)? {
				return Ok(true);
			}
		}

		Ok(false)
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

	/// Count protocol journal records for one run.
	pub fn event_count(&self, run_id: &str) -> Result<i64> {
		let state = self.lock()?;

		Ok(state.protocol_event_summary(run_id).event_count)
	}

	/// Read the latest recorded protocol-event timestamp for one run as a Unix epoch.
	pub fn last_protocol_activity_unix_epoch(&self, run_id: &str) -> Result<Option<i64>> {
		let state = self.lock()?;

		Ok(state.protocol_event_summary(run_id).last_event_at_unix)
	}

	pub(super) fn refresh_protocol_event_summaries_for_runs_locked(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.load_protocol_event_summaries_for_runs(state, run_ids)
	}

	fn rebuild_protocol_event_summaries_for_runs_locked(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.rebuild_protocol_event_summaries_for_runs(state, run_ids)
	}

	fn upsert_protocol_event_summary_locked(
		&self,
		run_id: &str,
		summary: &ProtocolEventSummaryRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_protocol_event_summary(run_id, summary)
	}

	fn append_protocol_event_locked(
		&self,
		run_id: &str,
		event: &ProtocolEventRecord,
	) -> Result<bool> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(true);
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.append_protocol_event(run_id, event)
	}

	fn protocol_event_locked(
		&self,
		run_id: &str,
		sequence_number: i64,
	) -> Result<Option<ProtocolEventRecord>> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(None);
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.protocol_event(run_id, sequence_number)
	}
}

fn protocol_event_payload_sha256(payload: &str) -> String {
	let digest = Sha256::digest(payload.as_bytes());
	let mut hash = String::with_capacity(64);

	for byte in digest {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	hash
}

fn protocol_event_is_terminal_thread_archive(event_type: &str) -> bool {
	TERMINAL_THREAD_ARCHIVE_EVENT_TYPES.contains(&event_type)
}

fn protocol_event_can_be_discarded_after_archive(event: &ProtocolEventRecord) -> bool {
	!protocol_event_is_terminal_thread_archive(&event.event_type)
		&& event.event_type != DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE
}

fn protocol_event_conflict_should_be_discarded_after_archive(
	existing: &ProtocolEventRecord,
	candidate: &ProtocolEventRecord,
) -> bool {
	protocol_event_is_terminal_thread_archive(&existing.event_type)
		&& protocol_event_can_be_discarded_after_archive(candidate)
}

fn protocol_event_is_discarded_post_archive_collision(
	existing: &ProtocolEventRecord,
	candidate: &ProtocolEventRecord,
) -> bool {
	existing.event_type == DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE
		&& candidate.event_type == DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE
		&& !existing.is_idempotent_replay_of(candidate)
}

fn discarded_post_archive_protocol_event(mut event: ProtocolEventRecord) -> ProtocolEventRecord {
	if event.event_type == DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE {
		return event;
	}

	event.sequence_number = discarded_post_archive_protocol_sequence(&event);
	event.event_type = DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE.to_owned();

	event
}

fn discarded_post_archive_protocol_event_with_log(
	run_id: &str,
	event: ProtocolEventRecord,
) -> ProtocolEventRecord {
	let original_sequence_number = event.sequence_number;
	let original_event_type = event.event_type.clone();
	let discarded = discarded_post_archive_protocol_event(event);

	tracing::info!(
		run_id,
		original_sequence_number,
		original_event_type,
		discarded_sequence_number = discarded.sequence_number,
		discarded_event_type = discarded.event_type.as_str(),
		"Discarded late app-server protocol event after terminal thread archive barrier; child protocol activity is isolated from parent journal and closeout state."
	);

	discarded
}

fn discarded_post_archive_protocol_sequence(event: &ProtocolEventRecord) -> i64 {
	let payload =
		format!("{}\n{}\n{}", event.sequence_number, event.event_type, event.payload_sha256);
	let digest = Sha256::digest(payload.as_bytes());
	let mut bytes = [0_u8; 8];

	bytes.copy_from_slice(&digest[..8]);

	let slot = i64::from_be_bytes(bytes) & i64::MAX;

	if slot == i64::MAX { i64::MIN } else { -1 - slot }
}

fn next_discarded_post_archive_sequence_after_collision(sequence_number: i64) -> Result<i64> {
	if sequence_number == i64::MIN {
		eyre::bail!("Post-archive discarded protocol event sequence space is exhausted.");
	}

	Ok(sequence_number - 1)
}

fn ensure_protocol_event_replay_matches(
	run_id: &str,
	existing: &ProtocolEventRecord,
	candidate: &ProtocolEventRecord,
) -> Result<()> {
	if existing.is_idempotent_replay_of(candidate) {
		return Ok(());
	}

	eyre::bail!(
		"Protocol event `{run_id}` sequence `{}` conflicts with an existing runtime journal event: \
		 existing event_type=`{}` payload_sha256=`{}`, candidate event_type=`{}` payload_sha256=`{}`.",
		candidate.sequence_number,
		existing.event_type,
		existing.payload_sha256,
		candidate.event_type,
		candidate.payload_sha256,
	);
}
