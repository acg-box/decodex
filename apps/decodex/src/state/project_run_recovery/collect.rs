mod finalize;
mod sources;

use std::collections::{BTreeMap, HashSet};

use crate::state::{
	Result, StateData, project_run_recovery::candidate::ProjectRunRecoveryCandidate,
};

pub(in crate::state) fn project_lease_run_ids(
	state: &StateData,
	project_id: &str,
	issue_id: Option<&str>,
) -> Vec<String> {
	state
		.leases
		.values()
		.filter(|lease| lease.project_id == project_id)
		.filter(|lease| issue_id.is_none_or(|issue_id| lease.issue_id == issue_id))
		.map(|lease| lease.run_id.clone())
		.collect()
}

pub(in crate::state) fn project_run_recovery_candidates(
	state: &StateData,
	project_id: &str,
	issue_id: Option<&str>,
) -> Result<Vec<ProjectRunRecoveryCandidate>> {
	let recorded_run_ids = state.run_attempts.keys().cloned().collect::<HashSet<_>>();
	let mut candidates = BTreeMap::<String, ProjectRunRecoveryCandidate>::new();

	sources::collect_control_channel_recovery_candidates(
		state,
		project_id,
		issue_id,
		&recorded_run_ids,
		&mut candidates,
	);
	sources::collect_private_event_recovery_candidates(
		state,
		project_id,
		issue_id,
		&recorded_run_ids,
		&mut candidates,
	);
	sources::collect_review_checkpoint_recovery_candidates(
		state,
		project_id,
		issue_id,
		&recorded_run_ids,
		&mut candidates,
	);
	sources::collect_lease_recovery_candidates(
		state,
		project_id,
		issue_id,
		&recorded_run_ids,
		&mut candidates,
	);
	sources::collect_worktree_marker_recovery_candidates(
		state,
		project_id,
		issue_id,
		&recorded_run_ids,
		&mut candidates,
	)?;
	finalize::finalize_project_run_recovery_candidates(state, &mut candidates);

	Ok(candidates.into_values().collect())
}
