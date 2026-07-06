use crate::orchestrator::{
	OperatorPostReviewLaneStatus, OperatorRunStatus, OperatorStatusSnapshot,
	OperatorWorktreeStatus, QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT, WorktreeOwnership,
	kernel::state::OwnershipState,
};

pub(crate) fn post_review_worktree_ownership(
	lane: &OperatorPostReviewLaneStatus,
) -> WorktreeOwnership {
	WorktreeOwnership {
		kind: "post_review_lane",
		reason: format!("Review & Landing owns this worktree as `{}`.", lane.classification),
		next_action: None,
		audit_required: false,
	}
}

pub(crate) fn worktree_current_lane_owner<'a>(
	worktree: &OperatorWorktreeStatus,
	snapshot: &'a OperatorStatusSnapshot,
) -> Option<&'a OperatorRunStatus> {
	snapshot.current_lanes.iter().chain(snapshot.recent_runs.iter()).find(|run| {
		matches!(
			OwnershipState::from_str(&run.ownership_state),
			Some(
				OwnershipState::LeasedRun
					| OwnershipState::RetainedAttention
					| OwnershipState::OrphanedLiveThread
					| OwnershipState::Terminalizing
					| OwnershipState::ContinuationPending
			)
		) && (run.worktree_path.as_deref() == Some(worktree.worktree_path.as_str())
			|| run.branch_name.as_deref() == Some(worktree.branch_name.as_str())
			|| run.issue_id == worktree.issue_id)
	})
}

pub(crate) fn worktree_post_review_owner<'a>(
	worktree: &OperatorWorktreeStatus,
	snapshot: &'a OperatorStatusSnapshot,
) -> Option<&'a OperatorPostReviewLaneStatus> {
	snapshot.post_review_lanes.iter().find(|lane| {
		lane.worktree_path == worktree.worktree_path
			|| lane.branch_name == worktree.branch_name
			|| lane.issue_id == worktree.issue_id
			|| lane.issue_identifier == worktree.issue_id
			|| worktree.issue_identifier.as_deref() == Some(lane.issue_identifier.as_str())
	})
}

pub(crate) fn worktree_has_queued_attention_owner(
	worktree: &OperatorWorktreeStatus,
	snapshot: &OperatorStatusSnapshot,
) -> bool {
	snapshot.queued_candidates.iter().any(|candidate| {
		matches!(
			candidate.reason.as_str(),
			"issue_needs_attention" | QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT
		) && (candidate.attention.as_ref().and_then(|attention| attention.worktree_path.as_deref())
			== Some(worktree.worktree_path.as_str())
			|| candidate.issue_id == worktree.issue_id
			|| candidate.issue_identifier == worktree.issue_id
			|| worktree.issue_identifier.as_deref() == Some(candidate.issue_identifier.as_str()))
	})
}
