use crate::orchestrator::{
	self, OperatorHistoryLaneStatus, OperatorStatusSnapshot, OperatorWorktreeStatus,
};

pub(crate) fn history_attention_worktree_next_action(lane: &OperatorHistoryLaneStatus) -> String {
	let Some(reason) = lane.ledger_outcome.needs_attention_reason.as_deref() else {
		return String::from(
			"inspect the retained worktree diff and resolve the terminal attention outcome manually",
		);
	};

	if lane.ledger_outcome.final_outcome == "terminal_failure"
		&& reason == "review_handoff_writeback_failed"
		&& let Some(issue_identifier) = lane.issue_identifier.as_deref()
	{
		return format!(
			"Run `decodex recover review-handoff diagnose {issue_identifier} --json` to verify retained PR lineage, then follow the reported rebind recovery command."
		);
	}

	reason.to_owned()
}

pub(crate) fn worktree_history_attention_owner<'a>(
	worktree: &OperatorWorktreeStatus,
	snapshot: &'a OperatorStatusSnapshot,
) -> Option<&'a OperatorHistoryLaneStatus> {
	let worktree_issue_key = orchestrator::operator_issue_attention_key(
		&worktree.issue_id,
		worktree.issue_identifier.as_deref(),
	);

	snapshot.history_lanes.iter().find(|lane| {
		orchestrator::history_ledger_outcome_requires_attention(&lane.ledger_outcome)
			&& (orchestrator::history_lane_group_key(lane) == worktree_issue_key
				|| lane.latest_run.worktree_path.as_deref()
					== Some(worktree.worktree_path.as_str())
				|| lane.latest_run.branch_name.as_deref() == Some(worktree.branch_name.as_str()))
	})
}
