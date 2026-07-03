use std::collections::HashSet;

use crate::{
	prelude::Result,
	state::{ProjectRunStatus, StateStore, project_run_recovery},
};

impl StateStore {
	/// List recent run attempts for one project, including lease and protocol summary fields.
	#[cfg(test)]
	pub fn list_recent_runs(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<ProjectRunStatus>> {
		self.list_project_runs(project_id, limit).map(|(_active, recent)| recent)
	}

	/// List active and recent run attempts for one project from one durable snapshot.
	pub(crate) fn list_project_runs(
		&self,
		project_id: &str,
		base_recent_limit: usize,
	) -> Result<(Vec<ProjectRunStatus>, Vec<ProjectRunStatus>)> {
		self.list_project_runs_with_mode(
			project_id,
			base_recent_limit,
			project_run_recovery::ProjectRunListingMode::AllowMarkerIdentityPersistence,
		)
	}

	/// List active and recent project runs without persisting marker-derived identities.
	pub(crate) fn list_project_runs_read_only(
		&self,
		project_id: &str,
		base_recent_limit: usize,
	) -> Result<(Vec<ProjectRunStatus>, Vec<ProjectRunStatus>)> {
		self.list_project_runs_with_mode(
			project_id,
			base_recent_limit,
			project_run_recovery::ProjectRunListingMode::ReadOnly,
		)
	}

	fn list_project_runs_with_mode(
		&self,
		project_id: &str,
		base_recent_limit: usize,
		mode: project_run_recovery::ProjectRunListingMode,
	) -> Result<(Vec<ProjectRunStatus>, Vec<ProjectRunStatus>)> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;

		if matches!(
			mode,
			project_run_recovery::ProjectRunListingMode::AllowMarkerIdentityPersistence
		) {
			self.refresh_run_attempt_identities_from_worktree_markers_locked(
				&mut state, project_id,
			)?;
		}

		self.refresh_project_loop_evidence_state_locked(&mut state, project_id)?;

		let lease_run_ids = project_run_recovery::project_lease_run_ids(&state, project_id, None);

		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &lease_run_ids)?;

		let recovery_candidates =
			project_run_recovery::project_run_recovery_candidates(&state, project_id, None)?
				.into_iter()
				.filter(|candidate| {
					project_run_recovery::project_run_recovery_candidate_counts_as_project_run(
						&state, candidate,
					)
				})
				.collect::<Vec<_>>();
		let recovery_run_ids = recovery_candidates
			.iter()
			.map(|candidate| candidate.run_id().to_owned())
			.collect::<Vec<_>>();

		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &recovery_run_ids)?;
		self.refresh_protocol_event_summaries_for_runs_locked(&mut state, &recovery_run_ids)?;

		let mut runs = state
			.run_attempts
			.values()
			.filter_map(|attempt| state.project_run_status(project_id, attempt))
			.collect::<Vec<_>>();

		runs.extend(recovery_candidates.iter().filter_map(|candidate| {
			project_run_recovery::project_run_status_from_recovery_candidate(&state, candidate)
		}));
		runs.sort_by(crate::state::compare_project_run_status);

		let leased_runs =
			runs.iter().filter(|status| status.run_lease()).cloned().collect::<Vec<_>>();
		let recent_limit = base_recent_limit.saturating_add(leased_runs.len());
		let recent_run_ids =
			runs.iter().take(recent_limit).map(|run| run.run_id().to_owned()).collect::<Vec<_>>();
		let mut summary_run_ids =
			leased_runs.iter().map(|run| run.run_id().to_owned()).collect::<Vec<_>>();

		summary_run_ids.extend(recent_run_ids);
		summary_run_ids.sort();
		summary_run_ids.dedup();
		self.refresh_protocol_event_summaries_for_runs_locked(&mut state, &summary_run_ids)?;
		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &summary_run_ids)?;

		let summary_run_id_set = summary_run_ids.iter().cloned().collect::<HashSet<_>>();
		let mut selected_runs = state
			.run_attempts
			.values()
			.filter(|attempt| summary_run_id_set.contains(&attempt.run_id))
			.filter_map(|attempt| state.project_run_status(project_id, attempt))
			.collect::<Vec<_>>();

		selected_runs.extend(
			recovery_candidates
				.iter()
				.filter(|candidate| summary_run_id_set.contains(candidate.run_id()))
				.filter_map(|candidate| {
					project_run_recovery::project_run_status_from_recovery_candidate(
						&state, candidate,
					)
				}),
		);
		selected_runs.sort_by(crate::state::compare_project_run_status);

		let leased_runs =
			selected_runs.iter().filter(|status| status.run_lease()).cloned().collect::<Vec<_>>();
		let mut recent_runs = selected_runs;

		recent_runs.truncate(recent_limit);

		Ok((leased_runs, recent_runs))
	}

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

	/// List all leased run attempts for one project without applying the recent-run limit.
	pub fn list_leased_runs(&self, project_id: &str) -> Result<Vec<ProjectRunStatus>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;

		let mut runs = state
			.run_attempts
			.values()
			.filter_map(|attempt| {
				let status = state.project_run_status(project_id, attempt)?;

				status.run_lease.then_some(status)
			})
			.collect::<Vec<_>>();
		let mut run_ids = runs.iter().map(|run| run.run_id().to_owned()).collect::<Vec<_>>();

		run_ids.sort();
		run_ids.dedup();
		self.refresh_protocol_event_summaries_for_runs_locked(&mut state, &run_ids)?;

		runs = state
			.run_attempts
			.values()
			.filter(|attempt| run_ids.contains(&attempt.run_id))
			.filter_map(|attempt| {
				let status = state.project_run_status(project_id, attempt)?;

				status.run_lease.then_some(status)
			})
			.collect::<Vec<_>>();

		runs.sort_by(crate::state::compare_project_run_status);

		Ok(runs)
	}
}
