use std::collections::HashSet;

use crate::orchestrator::{
	OperatorHistoryLaneStatus, OperatorHistoryLedgerOutcome, OperatorRunStatus,
	status_run_projection, status_summary,
};

pub(crate) fn current_lane_has_authoritative_live_owner(run: &OperatorRunStatus) -> bool {
	status_run_projection::operator_run_lane_control_readback(run).has_authoritative_live_owner
}

pub(crate) fn history_ledger_outcome_is_terminal(outcome: &OperatorHistoryLedgerOutcome) -> bool {
	outcome.ledger_status == "present"
		&& matches!(
			outcome.final_outcome.as_str(),
			"cleanup_complete" | "closeout" | "landed" | "needs_attention" | "terminal_failure"
		)
}

pub(crate) fn history_ledger_outcome_requires_attention(
	outcome: &OperatorHistoryLedgerOutcome,
) -> bool {
	outcome.ledger_status == "present"
		&& matches!(outcome.final_outcome.as_str(), "needs_attention" | "terminal_failure")
}

pub(crate) fn history_lane_group_key(lane: &OperatorHistoryLaneStatus) -> String {
	let issue_id = lane.issue_id.trim();

	if !issue_id.is_empty() && !issue_id.eq_ignore_ascii_case("unknown") {
		return issue_id.to_ascii_uppercase();
	}

	let issue_key = lane.issue_key.trim();

	if !issue_key.is_empty() && !issue_key.eq_ignore_ascii_case("unknown") {
		return issue_key.to_ascii_uppercase();
	}

	status_run_projection::operator_run_group_key(&lane.latest_run)
}

pub(crate) fn current_lane_terminal_outcome_supersedes(
	run: &OperatorRunStatus,
	outcome: &OperatorHistoryLedgerOutcome,
	completed_state: Option<&str>,
	current_attention_worktree_keys: &HashSet<String>,
) -> bool {
	if !history_ledger_outcome_is_terminal(outcome) {
		return false;
	}
	if current_lane_has_authoritative_live_owner(run) {
		return false;
	}
	if history_ledger_outcome_requires_attention(outcome) {
		return !current_lane_has_current_attention_signal(run, current_attention_worktree_keys);
	}

	current_lane_tracker_terminal_is_clean(run, completed_state)
}

pub(crate) fn history_ledger_outcome_supersedes_local_attempts(
	outcome: &OperatorHistoryLedgerOutcome,
) -> bool {
	history_ledger_outcome_is_terminal(outcome)
}

pub(crate) fn terminal_attention_queue_key(issue_id: &str, issue_identifier: &str) -> String {
	status_summary::operator_issue_attention_key(issue_id, Some(issue_identifier))
}

fn current_lane_has_current_attention_signal(
	run: &OperatorRunStatus,
	current_attention_worktree_keys: &HashSet<String>,
) -> bool {
	run.needs_attention_label_present == Some(true)
		|| run.active_label_present == Some(true)
		|| current_attention_worktree_keys
			.contains(&status_run_projection::operator_run_group_key(run))
}

fn current_lane_tracker_terminal_is_clean(
	run: &OperatorRunStatus,
	completed_state: Option<&str>,
) -> bool {
	let has_tracker_metadata = run.issue_state.is_some()
		|| run.active_label_present.is_some()
		|| run.needs_attention_label_present.is_some();

	if !has_tracker_metadata {
		return true;
	}

	let Some(completed_state) = completed_state else {
		return false;
	};

	run.issue_state.as_deref() == Some(completed_state)
		&& run.active_label_present == Some(false)
		&& run.needs_attention_label_present == Some(false)
}
