use std::sync::MutexGuard;

use crate::{
	prelude::{Result, eyre},
	state::store::{self, StateData, StateStore},
};

impl StateStore {
	pub(in crate::state) fn lock_without_refresh(&self) -> Result<MutexGuard<'_, StateData>> {
		self.inner.lock().map_err(|_| eyre::eyre!("StateStore mutex is poisoned."))
	}

	pub(in crate::state) fn lock(&self) -> Result<MutexGuard<'_, StateData>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_runtime_state_locked(&mut state)?;

		Ok(state)
	}

	pub(in crate::state) fn refresh_runtime_state_locked(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;
		let loaded = sqlite.load_state()?;

		state.replace_durable_state(loaded);

		Ok(())
	}

	pub(in crate::state) fn refresh_project_run_metadata_state_locked(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;
		let loaded = sqlite.load_project_run_metadata_for_project(project_id)?;

		state.replace_project_run_metadata_state(loaded);

		Ok(())
	}

	pub(in crate::state) fn refresh_run_activity_summaries_for_runs_locked(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.load_run_activity_summaries_for_runs(state, run_ids)
	}

	pub(in crate::state) fn refresh_run_attempt_identities_from_worktree_markers_locked(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let updates = state
			.worktrees
			.values()
			.filter(|mapping| mapping.project_id == project_id)
			.filter_map(|mapping| {
				let marker = match store::read_run_activity_marker_snapshot(&mapping.worktree_path)
				{
					Ok(Some(marker)) => marker,
					Ok(None) => return None,
					Err(_) => return None,
				};
				let attempt = state.run_attempts.get(marker.run_id())?;

				if attempt.issue_id != mapping.issue_id
					|| attempt.attempt_number != marker.attempt_number()
				{
					return None;
				}

				let thread_id = marker.thread_id().map(str::to_owned);
				let turn_id = marker.turn_id().map(str::to_owned);

				if thread_id.is_none() && turn_id.is_none() {
					return None;
				}

				Some(Ok((marker.run_id().to_owned(), thread_id, turn_id)))
			})
			.collect::<Result<Vec<_>>>()?;

		for (run_id, thread_id, turn_id) in updates {
			let Some(attempt) = state.run_attempts.get_mut(&run_id) else {
				continue;
			};
			let mut changed = false;

			if attempt.thread_id.is_none()
				&& let Some(thread_id) = thread_id
			{
				attempt.thread_id = Some(thread_id);
				changed = true;
			}
			if attempt.turn_id.is_none()
				&& let Some(turn_id) = turn_id
			{
				attempt.turn_id = Some(turn_id);
				changed = true;
			}
			if changed {
				let attempt = attempt.clone();

				self.upsert_run_attempt_locked(&attempt)?;
			}
		}

		Ok(())
	}

	pub(in crate::state) fn refresh_project_loop_evidence_state_locked(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;
		let loaded = sqlite.load_project_loop_evidence_for_project(project_id)?;

		state.replace_project_loop_evidence_state(project_id, loaded);

		Ok(())
	}

	pub(in crate::state) fn refresh_project_registry_state_locked(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;
		let loaded = sqlite.load_project_registry_state()?;

		state.replace_project_registry_state(loaded);

		Ok(())
	}
}
