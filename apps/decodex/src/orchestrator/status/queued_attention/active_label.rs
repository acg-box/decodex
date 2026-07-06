use crate::{
	orchestrator::{
		ATTENTION_ERROR_EVIDENCE_MISSING, QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT,
		WorktreeTrackedChangeState,
	},
	state::RunActivityMarker,
};

pub(crate) fn operator_active_label_attention_summary(
	reason: &str,
	marker: Option<&RunActivityMarker>,
	retry_budget_attempts: i64,
	worktree_tracked_change_state: WorktreeTrackedChangeState,
	attention_error_class: Option<&str>,
) -> Option<String> {
	if reason != QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT {
		return None;
	}
	if worktree_tracked_change_state.has_tracked_changes() {
		return Some(String::from(
			"Linear active ownership is still present with retained worktree changes; inspect the patch and reconcile the lane before dispatch.",
		));
	}
	if worktree_tracked_change_state.is_unknown() {
		return Some(String::from(
			"Linear active ownership is still present and retained worktree cleanliness could not be verified; inspect the worktree before dispatch.",
		));
	}
	if retry_budget_attempts > 0 {
		return Some(format!(
			"Retryable failed-start cleanup is still pending after {retry_budget_attempts} failed attempts; no retained worktree changes were found, so clear stale active ownership before dispatch."
		));
	}
	if attention_error_class == Some(ATTENTION_ERROR_EVIDENCE_MISSING) {
		return Some(if marker.is_some() {
			String::from(
				"Linear active ownership is still present but private execution evidence is missing; inspect the retained marker and reconcile before dispatch.",
			)
		} else {
			String::from(
				"Linear active ownership is still present but the retained marker or private execution evidence is missing; reconcile before dispatch.",
			)
		});
	}
	if marker.is_some() {
		return Some(String::from(
			"Linear active ownership is still present alongside queue intake; inspect the retained marker before dispatch.",
		));
	}

	Some(String::from(
		"Linear active ownership is still present without a matching local run lease; reconcile before dispatch.",
	))
}

pub(crate) fn operator_active_label_attention_next_action(
	reason: &str,
	issue_identifier: &str,
	worktree_tracked_change_state: WorktreeTrackedChangeState,
	attention_error_class: Option<&str>,
) -> Option<String> {
	if reason != QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT {
		return None;
	}
	if worktree_tracked_change_state.has_tracked_changes()
		|| worktree_tracked_change_state.is_unknown()
	{
		return Some(String::from(
			"inspect_retained_worktree_changes_before_stale_active_recovery",
		));
	}
	if attention_error_class == Some(ATTENTION_ERROR_EVIDENCE_MISSING) {
		return Some(format!(
			"run_stale_active_recovery: decodex recover stale-active diagnose {issue_identifier} --json; decodex recover stale-active release {issue_identifier} --dry-run"
		));
	}

	Some(format!(
		"run_stale_active_recovery: decodex recover stale-active diagnose {issue_identifier}; decodex recover stale-active release {issue_identifier} --dry-run"
	))
}
