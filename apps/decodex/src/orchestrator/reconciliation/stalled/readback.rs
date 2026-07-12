use crate::orchestrator::reconciliation::{
	self, Result, RunAttempt, RunLeaseDisposition, StateStore, WorktreeMapping,
};

pub(crate) fn stalled_run_has_retained_partial_progress(
	worktree_mapping: Option<&WorktreeMapping>,
) -> bool {
	match worktree_mapping {
		Some(mapping) => reconciliation::worktree_has_tracked_changes(mapping.worktree_path()),
		None => false,
	}
}

pub(crate) fn retained_review_handoff_matches_run(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<bool> {
	let Some(worktree_mapping) = worktree_mapping else {
		return Ok(false);
	};
	let Some(record) = state_store.review_lifecycle_record(
		worktree_mapping.project_id(),
		run_attempt.issue_id(),
		worktree_mapping.branch_name(),
	)?
	else {
		return Ok(false);
	};

	Ok(record.run_id() == run_attempt.run_id()
		&& record.attempt_number() == run_attempt.attempt_number()
		&& record.branch_name() == worktree_mapping.branch_name())
}

pub(crate) fn superseded_run_disposition(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
) -> Result<Option<RunLeaseDisposition>> {
	let Some(project_id) = run_attempt.project_id() else {
		return Ok(None);
	};
	let Some(latest_attempt) =
		state_store.latest_run_attempt_for_lane(project_id, run_attempt.issue_id())?
	else {
		return Ok(None);
	};

	if latest_attempt.attempt_number() <= run_attempt.attempt_number() {
		return Ok(None);
	}

	Ok(Some(RunLeaseDisposition::Superseded {
		newer_run_id: latest_attempt.run_id().to_owned(),
		newer_attempt_number: latest_attempt.attempt_number(),
	}))
}
