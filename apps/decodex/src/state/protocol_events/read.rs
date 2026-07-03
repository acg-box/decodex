use crate::{
	prelude::{Result, eyre},
	state::{
		StateData, StateStore,
		protocol_events::archive::{self, TERMINAL_THREAD_ARCHIVE_EVENT_TYPES},
	},
};

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

	pub(super) fn run_has_terminal_thread_archive_event_locked(
		&self,
		state: &StateData,
		run_id: &str,
	) -> Result<bool> {
		if state.events.get(run_id).is_some_and(|events| {
			events
				.iter()
				.any(|event| archive::protocol_event_is_terminal_thread_archive(&event.event_type))
		}) {
			return Ok(true);
		}
		if state.event_summaries.get(run_id).is_some_and(|summary| {
			summary
				.last_event_type
				.as_deref()
				.is_some_and(archive::protocol_event_is_terminal_thread_archive)
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
}
