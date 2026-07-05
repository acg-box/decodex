use crate::{
	prelude::Result,
	state::{ProjectRunStatus, StateStore, project_run_recovery},
};

impl StateStore {
	/// List all locally recorded run attempts for one issue in one project.
	pub(crate) fn list_project_issue_runs(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<Vec<ProjectRunStatus>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;
		self.refresh_run_attempt_identities_from_worktree_markers_locked(&mut state, project_id)?;
		self.refresh_project_loop_evidence_state_locked(&mut state, project_id)?;

		let lease_run_ids =
			project_run_recovery::project_lease_run_ids(&state, project_id, Some(issue_id));

		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &lease_run_ids)?;

		let recovery_candidates = project_run_recovery::project_run_recovery_candidates(
			&state,
			project_id,
			Some(issue_id),
		)?;
		let recovery_run_ids = recovery_candidates
			.iter()
			.map(|candidate| candidate.run_id().to_owned())
			.collect::<Vec<_>>();
		let run_ids = state
			.run_attempts
			.values()
			.filter(|attempt| attempt.issue_id == issue_id)
			.map(|attempt| attempt.run_id.clone())
			.collect::<Vec<_>>();
		let mut summary_run_ids = run_ids;

		summary_run_ids.extend(recovery_run_ids);
		summary_run_ids.sort();
		summary_run_ids.dedup();
		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &summary_run_ids)?;
		self.refresh_protocol_event_summaries_for_runs_locked(&mut state, &summary_run_ids)?;

		let mut runs = state
			.run_attempts
			.values()
			.filter(|attempt| attempt.issue_id == issue_id)
			.filter_map(|attempt| state.project_run_status(project_id, attempt))
			.collect::<Vec<_>>();

		runs.extend(recovery_candidates.iter().filter_map(|candidate| {
			project_run_recovery::project_run_status_from_recovery_candidate(&state, candidate)
		}));
		runs.sort_by(crate::state::compare_project_run_status);

		Ok(runs)
	}
}
