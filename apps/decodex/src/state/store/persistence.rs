use std::sync::MutexGuard;

use crate::prelude::{Result, eyre};

use super::{
	super::runtime_records::{
		AutonomyObjectiveRuntimeRecord, AutonomyProposalRuntimeRecord, AutonomySignalRuntimeRecord,
		DecisionContractRuntimeRecord, ExecutionProgramRuntimeRecord,
		LinearExecutionEventRuntimeRecord, PrivateExecutionEventRuntimeRecord,
		RunActivitySummaryRecord, RunAttemptRecord, RunControlChannelRecord, WorktreeMappingRecord,
	},
	IssueLease, ProjectRegistration, StateData, StateStore, read_run_activity_marker_snapshot,
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
				let marker = match read_run_activity_marker_snapshot(&mapping.worktree_path) {
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

	pub(in crate::state) fn persist_runtime_state_locked(&self, state: &StateData) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.persist_runtime_state(state)
	}

	pub(in crate::state) fn delete_project_locked(&self, service_id: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_project(service_id)
	}

	pub(in crate::state) fn upsert_project_locked(
		&self,
		project: &ProjectRegistration,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_project(project)
	}

	pub(in crate::state) fn delete_connector_backoff_locked(
		&self,
		project_id: &str,
		connector: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_connector_backoff(project_id, connector)
	}

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

	pub(in crate::state) fn insert_linear_execution_event_if_absent_locked(
		&self,
		record: &LinearExecutionEventRuntimeRecord,
	) -> Result<bool> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(true);
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.insert_linear_execution_event_if_absent(record)
	}

	pub(in crate::state) fn delete_linear_execution_event_locked(
		&self,
		idempotency_key: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_linear_execution_event(idempotency_key)
	}

	pub(in crate::state) fn list_persisted_linear_execution_events(
		&self,
		service_id: &str,
		issue_id: &str,
	) -> Result<Option<Vec<LinearExecutionEventRuntimeRecord>>> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(None);
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.list_linear_execution_events(service_id, issue_id).map(Some)
	}

	pub(in crate::state) fn insert_private_execution_event_locked(
		&self,
		record: &PrivateExecutionEventRuntimeRecord,
	) -> Result<Option<i64>> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(None);
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.insert_private_execution_event(record).map(Some)
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_decision_contract_locked(
		&self,
		record: &DecisionContractRuntimeRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_decision_contract(record)
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_autonomy_objective_locked(
		&self,
		record: &AutonomyObjectiveRuntimeRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_autonomy_objective(record)
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_autonomy_signal_locked(
		&self,
		record: &AutonomySignalRuntimeRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_autonomy_signal(record)
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_autonomy_proposal_locked(
		&self,
		record: &AutonomyProposalRuntimeRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_autonomy_proposal(record)
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_execution_program_locked(
		&self,
		record: &ExecutionProgramRuntimeRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_execution_program(record)
	}

	pub(in crate::state) fn delete_lease_locked(&self, issue_id: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_lease(issue_id)
	}

	pub(in crate::state) fn retarget_issue_identity_locked(
		&self,
		previous_issue_id: &str,
		canonical_issue_id: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.retarget_issue_identity(previous_issue_id, canonical_issue_id)
	}

	pub(in crate::state) fn delete_worktree_and_review_lifecycle_locked(
		&self,
		issue_id: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_worktree_and_review_lifecycle(issue_id)
	}

	pub(in crate::state) fn delete_worktree_mapping_locked(&self, issue_id: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_worktree_mapping(issue_id)
	}

	pub(in crate::state) fn delete_review_marker_identity_locked(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_review_marker_identity(
			project_id,
			issue_id,
			branch_name,
			run_id,
			attempt_number,
		)
	}

	pub(in crate::state) fn delete_review_policy_checkpoints_for_run_attempt_locked(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_review_policy_checkpoints_for_run_attempt(
			project_id,
			issue_id,
			run_id,
			attempt_number,
		)
	}

	pub(in crate::state) fn delete_loop_guardrail_checkpoints_for_issue_locked(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_loop_guardrail_checkpoints_for_issue(project_id, issue_id)
	}

	pub(in crate::state) fn delete_loop_guardrail_checkpoint_locked(
		&self,
		project_id: &str,
		issue_id: &str,
		reason: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_loop_guardrail_checkpoint(project_id, issue_id, reason)
	}
}
