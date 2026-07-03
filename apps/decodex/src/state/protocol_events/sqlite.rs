use crate::{
	prelude::{Result, eyre},
	state::{
		StateData, StateStore,
		runtime_records::{ProtocolEventRecord, ProtocolEventSummaryRecord},
	},
};

impl StateStore {
	pub(in crate::state) fn refresh_protocol_event_summaries_for_runs_locked(
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

	pub(super) fn rebuild_protocol_event_summaries_for_runs_locked(
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

	pub(super) fn upsert_protocol_event_summary_locked(
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

	pub(super) fn append_protocol_event_locked(
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

	pub(super) fn protocol_event_locked(
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
