mod attention;
mod cleanup;
mod project;
mod queue;
mod run_state;

use crate::orchestrator::{OperatorQueuedIssueStatus, OperatorRunStatus, OperatorStatusSnapshot};

pub(in crate::orchestrator) fn refresh_operator_project_summary(
	snapshot: &mut OperatorStatusSnapshot,
	completed_state: Option<&str>,
) {
	project::refresh_operator_project_summary(snapshot, completed_state);
}

pub(in crate::orchestrator) fn operator_run_counts_as_waiting(run: &OperatorRunStatus) -> bool {
	run_state::operator_run_counts_as_waiting(run)
}

pub(in crate::orchestrator) fn queued_candidate_counts_as_waiting_intake(
	candidate: &OperatorQueuedIssueStatus,
) -> bool {
	queue::queued_candidate_counts_as_waiting_intake(candidate)
}

pub(in crate::orchestrator) fn project_attention_count(
	snapshot: &OperatorStatusSnapshot,
	completed_state: Option<&str>,
) -> usize {
	attention::project_attention_count(snapshot, completed_state)
}

pub(in crate::orchestrator) fn project_history_only_attention_count(
	snapshot: &OperatorStatusSnapshot,
) -> usize {
	attention::project_history_only_attention_count(snapshot)
}

pub(in crate::orchestrator) fn operator_issue_attention_key(
	issue_id: &str,
	issue_identifier: Option<&str>,
) -> String {
	attention::operator_issue_attention_key(issue_id, issue_identifier)
}

pub(in crate::orchestrator) fn hydrate_post_review_lane_current_lane_shadowing(
	snapshot: &mut OperatorStatusSnapshot,
) {
	attention::hydrate_post_review_lane_current_lane_shadowing(snapshot);
}

pub(in crate::orchestrator) fn operator_run_counts_as_current_lane(
	run: &OperatorRunStatus,
) -> bool {
	run_state::operator_run_counts_as_current_lane(run)
}

pub(in crate::orchestrator) fn operator_run_has_live_execution(run: &OperatorRunStatus) -> bool {
	run_state::operator_run_has_live_execution(run)
}

pub(in crate::orchestrator) fn operator_run_counts_as_running(run: &OperatorRunStatus) -> bool {
	run_state::operator_run_counts_as_running(run)
}

pub(in crate::orchestrator) fn operator_run_counts_as_attention(run: &OperatorRunStatus) -> bool {
	run_state::operator_run_counts_as_attention(run)
}

pub(in crate::orchestrator) fn operator_run_has_recent_app_server_execution(
	run: &OperatorRunStatus,
) -> bool {
	run_state::operator_run_has_recent_app_server_execution(run)
}

pub(in crate::orchestrator) fn operator_run_has_stale_execution_without_known_process(
	run: &OperatorRunStatus,
) -> bool {
	run_state::operator_run_has_stale_execution_without_known_process(run)
}
