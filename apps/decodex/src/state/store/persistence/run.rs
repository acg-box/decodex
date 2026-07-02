use crate::{
	prelude::{Result, eyre},
	state::{
		runtime_records::{
			RunActivitySummaryRecord, RunAttemptRecord, RunControlChannelRecord,
			WorktreeMappingRecord,
		},
		store::{IssueLease, StateStore},
	},
};

impl StateStore {
	pub(in crate::state) fn upsert_run_attempt_locked(
		&self,
		attempt: &RunAttemptRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_run_attempt(attempt)
	}

	pub(in crate::state) fn upsert_run_control_channel_locked(
		&self,
		channel: &RunControlChannelRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_run_control_channel(channel)
	}

	pub(in crate::state) fn upsert_run_activity_summary_locked(
		&self,
		summary: &RunActivitySummaryRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_run_activity_summary(summary)
	}

	pub(in crate::state) fn upsert_lease_and_remember_run_project_locked(
		&self,
		lease: &IssueLease,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_lease_and_remember_run_project(lease)
	}

	pub(in crate::state) fn upsert_worktree_and_remember_run_project_locked(
		&self,
		mapping: &WorktreeMappingRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_worktree_and_remember_run_project(mapping)
	}
}
