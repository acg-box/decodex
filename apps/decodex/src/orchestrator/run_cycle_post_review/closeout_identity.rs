use std::path::Path;

use crate::{
	orchestrator::{
		RetainedReviewRunIdentity, StateStore, TERMINAL_GUARDED_RUN_STATUS, TrackerIssue,
		WorktreeMapping,
	},
	prelude::Result,
	state,
};

pub(crate) fn retained_closeout_preferred_run_identity(
	state_store: &StateStore,
	project_id: &str,
	issue: &TrackerIssue,
) -> Result<Option<RetainedReviewRunIdentity>> {
	let Some(worktree) = state_store.worktree_for_issue(&issue.id)? else {
		return Ok(None);
	};
	let Some(review_handoff) =
		state_store.review_handoff_marker(project_id, &issue.id, worktree.branch_name())?
	else {
		return Ok(None);
	};
	let identity = RetainedReviewRunIdentity {
		run_id: review_handoff.run_id().to_owned(),
		attempt_number: review_handoff.attempt_number(),
	};

	if retained_closeout_run_identity_is_reusable(state_store, &issue.id, &identity)?
		|| retained_closeout_handoff_identity_is_reusable_after_parent_reconciliation(
			state_store,
			&issue.id,
			&identity,
			&worktree,
		)? {
		return Ok(Some(identity));
	}

	Ok(None)
}

pub(crate) fn retained_closeout_run_identity_is_reusable(
	state_store: &StateStore,
	issue_id: &str,
	identity: &RetainedReviewRunIdentity,
) -> Result<bool> {
	if state_store.issue_has_retry_budget_attempt_after(issue_id, identity.attempt_number)? {
		return Ok(false);
	}

	let Some(existing_attempt) = state_store.run_attempt(&identity.run_id)? else {
		return Ok(true);
	};

	if existing_attempt.issue_id() != issue_id
		|| existing_attempt.attempt_number() != identity.attempt_number
	{
		return Ok(false);
	}

	Ok(!matches!(existing_attempt.status(), "failed" | "interrupted" | TERMINAL_GUARDED_RUN_STATUS))
}

fn retained_closeout_handoff_identity_is_reusable_after_parent_reconciliation(
	state_store: &StateStore,
	issue_id: &str,
	identity: &RetainedReviewRunIdentity,
	worktree: &WorktreeMapping,
) -> Result<bool> {
	if state_store.issue_has_retry_budget_attempt_after(issue_id, identity.attempt_number)? {
		return Ok(false);
	}

	let Some(existing_attempt) = state_store.run_attempt(&identity.run_id)? else {
		return Ok(false);
	};

	if existing_attempt.issue_id() != issue_id
		|| existing_attempt.attempt_number() != identity.attempt_number
	{
		return Ok(false);
	}
	if !matches!(existing_attempt.status(), "failed" | "interrupted") {
		return Ok(false);
	}
	if worktree_has_retry_schedule_for_run(worktree.worktree_path(), identity)? {
		return Ok(false);
	}

	Ok(true)
}

fn worktree_has_retry_schedule_for_run(
	worktree_path: &Path,
	identity: &RetainedReviewRunIdentity,
) -> Result<bool> {
	let Some(marker) = state::read_run_activity_marker_snapshot(worktree_path)? else {
		return Ok(false);
	};

	Ok(marker.run_id() == identity.run_id
		&& marker.attempt_number() == identity.attempt_number
		&& marker.retry_kind().is_some())
}
