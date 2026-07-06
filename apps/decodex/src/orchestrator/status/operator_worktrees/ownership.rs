mod history;
mod local_cleanup;
mod owners;
mod state;

use crate::{
	orchestrator::{OperatorStatusSnapshot, OperatorWorktreeStatus, WorktreeOwnership},
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
	let post_review_owner = owners::worktree_post_review_owner(worktree, snapshot);

	if let Some(run) = owners::worktree_current_lane_owner(worktree, snapshot) {
		return state::current_lane_worktree_ownership(run, post_review_owner);
	}
	if let Some(lane) = post_review_owner {
		return owners::post_review_worktree_ownership(lane);
	}
	if let Some(lane) = history::worktree_history_attention_owner(worktree, snapshot) {
		return WorktreeOwnership {
			kind: "retained_attention",
			reason: format!(
				"Run Ledger owns this worktree through terminal `{}` outcome.",
				lane.ledger_outcome.final_outcome
			),
			next_action: Some(history::history_attention_worktree_next_action(lane)),
			audit_required: false,
		};
	}

	if owners::worktree_has_queued_attention_owner(worktree, snapshot) {
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
		reason: local_cleanup::worktree_cleanup_only_reason(worktree, completed_state),
		next_action: audit_required.then(|| local_cleanup::legacy_cleanup_next_action(worktree)),
		audit_required,
	}
}
