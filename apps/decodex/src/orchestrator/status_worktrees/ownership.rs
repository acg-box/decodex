use crate::{
	orchestrator::{
		self, OperatorHistoryLaneStatus, OperatorPostReviewLaneStatus, OperatorRunStatus,
		OperatorStatusSnapshot, OperatorWorktreeStatus, QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT,
		WorktreeOwnership, kernel::state::OwnershipState,
	},
	state::WORKTREE_PROVENANCE_LEGACY_UNKNOWN,
};

pub(crate) fn refresh_worktree_ownership(
	snapshot: &mut OperatorStatusSnapshot,
	completed_state: Option<&str>,
) {
	let ownership = snapshot
		.worktrees
		.iter()
		.map(|worktree| worktree_ownership(worktree, snapshot, completed_state))
		.collect::<Vec<_>>();

	for (worktree, ownership) in snapshot.worktrees.iter_mut().zip(ownership) {
		worktree.ownership = ownership.kind.to_owned();
		worktree.ownership_reason = ownership.reason;
		worktree.recovery_next_action = ownership.next_action;
		worktree.provenance.audit_required = ownership.audit_required;
	}
}

fn worktree_ownership(
	worktree: &OperatorWorktreeStatus,
	snapshot: &OperatorStatusSnapshot,
	completed_state: Option<&str>,
) -> WorktreeOwnership {
	let post_review_owner = worktree_post_review_owner(worktree, snapshot);

	if let Some(run) = worktree_current_lane_owner(worktree, snapshot) {
		let ownership_state = OwnershipState::from_str(&run.ownership_state);

		if ownership_state == Some(OwnershipState::OrphanedLiveThread)
			&& let Some(lane) = post_review_owner
		{
			return post_review_worktree_ownership(lane);
		}

		return match ownership_state {
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
				reason: format!(
					"Lane `{}` has live evidence but no active Decodex lease.",
					run.run_id
				),
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
		};
	}
	if let Some(lane) = post_review_owner {
		return post_review_worktree_ownership(lane);
	}
	if let Some(lane) = worktree_history_attention_owner(worktree, snapshot) {
		return WorktreeOwnership {
			kind: "retained_attention",
			reason: format!(
				"Run Ledger owns this worktree through terminal `{}` outcome.",
				lane.ledger_outcome.final_outcome
			),
			next_action: Some(lane.ledger_outcome.needs_attention_reason.clone().unwrap_or_else(
				|| {
					String::from(
						"inspect the retained worktree diff and resolve the terminal attention outcome manually",
					)
				},
			)),
			audit_required: false,
		};
	}

	if worktree_has_queued_attention_owner(worktree, snapshot) {
		return WorktreeOwnership {
			kind: "queued_attention",
			reason: String::from(
				"Intake Queue owns this worktree because the issue needs operator attention.",
			),
			next_action: None,
			audit_required: false,
		};
	}

	if let Some(hygiene) = &worktree.hygiene {
		return WorktreeOwnership {
			kind: "post_land_cleanup",
			reason: hygiene.reason.clone(),
			next_action: Some(String::from(
				"inspect the merged worktree, preserve or discard local changes intentionally, then remove the linked worktree",
			)),
			audit_required: false,
		};
	}

	let audit_required = worktree.provenance.source == WORKTREE_PROVENANCE_LEGACY_UNKNOWN;

	WorktreeOwnership {
		kind: "cleanup_only",
		reason: worktree_cleanup_only_reason(worktree, completed_state),
		next_action: audit_required.then(|| legacy_cleanup_next_action(worktree)),
		audit_required,
	}
}

fn post_review_worktree_ownership(lane: &OperatorPostReviewLaneStatus) -> WorktreeOwnership {
	WorktreeOwnership {
		kind: "post_review_lane",
		reason: format!("Review & Landing owns this worktree as `{}`.", lane.classification),
		next_action: None,
		audit_required: false,
	}
}

fn worktree_current_lane_owner<'a>(
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

fn worktree_post_review_owner<'a>(
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

fn worktree_has_queued_attention_owner(
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

fn worktree_history_attention_owner<'a>(
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

fn worktree_cleanup_only_reason(
	worktree: &OperatorWorktreeStatus,
	completed_state: Option<&str>,
) -> String {
	if worktree.provenance.source == WORKTREE_PROVENANCE_LEGACY_UNKNOWN {
		return String::from(
			"Legacy worktree mapping has no durable runtime provenance; no active, queued, or post-review lane owns it, so Decodex cannot automatically prove PR or closeout lineage.",
		);
	}

	if let (Some(issue_state), Some(completed_state)) =
		(worktree.issue_state.as_deref(), completed_state)
		&& issue_state == completed_state
	{
		return format!(
			"Issue is {completed_state}; no active or post-review lane owns this worktree, so it is local cleanup only."
		);
	}

	String::from(
		"No current lane, queued recovery, or post-review lane owns this worktree; local cleanup only.",
	)
}

fn legacy_cleanup_next_action(worktree: &OperatorWorktreeStatus) -> String {
	let issue = worktree.issue_identifier.as_deref().unwrap_or(&worktree.issue_id);

	format!(
		"verify tracker/PR terminal state and clean git status for `{}`, then run `decodex recover legacy-closeout {issue} --pr <MERGED_PR> --dry-run`; rerun with `--manual-authority` before removing this worktree",
		worktree.worktree_path
	)
}
