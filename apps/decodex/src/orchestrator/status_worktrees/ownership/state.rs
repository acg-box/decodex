use crate::orchestrator::{
	OperatorPostReviewLaneStatus, OperatorRunStatus, WorktreeOwnership,
	kernel::state::OwnershipState, status_worktrees::ownership::owners,
};

pub(crate) fn current_lane_worktree_ownership(
	run: &OperatorRunStatus,
	post_review_owner: Option<&OperatorPostReviewLaneStatus>,
) -> WorktreeOwnership {
	let ownership_state = OwnershipState::from_str(&run.ownership_state);

	if ownership_state == Some(OwnershipState::OrphanedLiveThread)
		&& let Some(lane) = post_review_owner
	{
		return owners::post_review_worktree_ownership(lane);
	}

	match ownership_state {
		Some(OwnershipState::LeasedRun) => WorktreeOwnership {
			kind: "current_lane",
			reason: format!("Current lane `{}` owns this worktree.", run.run_id),
			next_action: None,
			audit_required: false,
		},
		Some(OwnershipState::RetainedAttention) => WorktreeOwnership {
			kind: "retained_attention",
			reason: format!(
				"Lane `{}` requires operator attention before it can own this worktree.",
				run.run_id
			),
			next_action: Some(run.lane_control_next_action.clone()),
			audit_required: true,
		},
		Some(OwnershipState::OrphanedLiveThread) => WorktreeOwnership {
			kind: "orphaned_live_thread",
			reason: format!("Lane `{}` has live evidence but no active Decodex lease.", run.run_id),
			next_action: Some(run.lane_control_next_action.clone()),
			audit_required: true,
		},
		Some(OwnershipState::Terminalizing) => WorktreeOwnership {
			kind: "terminalizing_lane",
			reason: format!(
				"Lane `{}` is inside terminalization and no longer counts as running.",
				run.run_id
			),
			next_action: Some(run.lane_control_next_action.clone()),
			audit_required: true,
		},
		Some(OwnershipState::ContinuationPending) => WorktreeOwnership {
			kind: "continuation_pending",
			reason: format!(
				"Lane `{}` is waiting for scheduled continuation re-entry.",
				run.run_id
			),
			next_action: Some(run.lane_control_next_action.clone()),
			audit_required: false,
		},
		_ => WorktreeOwnership {
			kind: "orphaned_local_worktree",
			reason: format!("Lane `{}` is not an active owner for this worktree.", run.run_id),
			next_action: Some(run.lane_control_next_action.clone()),
			audit_required: true,
		},
	}
}
