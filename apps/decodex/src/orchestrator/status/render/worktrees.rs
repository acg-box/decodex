use crate::orchestrator::{
	self, OperatorStatusSnapshot, OperatorWorktreeStatus, QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT,
};

pub(crate) fn rendered_recovery_worktrees(
	snapshot: &OperatorStatusSnapshot,
) -> Vec<(&str, &OperatorWorktreeStatus)> {
	let mut rendered_worktrees = snapshot
		.worktrees
		.iter()
		.map(|worktree| (rendered_worktree_role(worktree, snapshot), worktree))
		.filter(|(role, _)| rendered_worktree_role_rank(role) > 0)
		.collect::<Vec<_>>();

	rendered_worktrees.sort_by(|(left_role, left), (right_role, right)| {
		rendered_worktree_role_rank(left_role)
			.cmp(&rendered_worktree_role_rank(right_role))
			.then_with(|| left.issue_id.cmp(&right.issue_id))
			.then_with(|| left.branch_name.cmp(&right.branch_name))
			.then_with(|| left.worktree_path.cmp(&right.worktree_path))
	});

	rendered_worktrees
}

pub(super) fn append_rendered_recovery_worktrees(
	output: &mut String,
	rendered_worktrees: &[(&str, &OperatorWorktreeStatus)],
	hides_owned_worktrees: bool,
) {
	if rendered_worktrees.is_empty() {
		if hides_owned_worktrees {
			output.push_str("- none (owned worktrees are shown in their lane sections above)\n");
		} else {
			output.push_str("- none\n");
		}

		return;
	}

	for (role, worktree) in rendered_worktrees {
		output.push_str(&format!(
			"- issue_id: {}\n  issue: {}\n  state: {}\n  role: {}\n  reason: {}\n  branch: {}\n  worktree_path: {}\n  provenance_source: {}\n  provenance_created_at_unix: {}\n  provenance_updated_at_unix: {}\n  audit_required: {}\n  recovery_next_action: {}\n",
			worktree.issue_id,
			worktree.issue_identifier.as_deref().unwrap_or("none"),
			worktree.issue_state.as_deref().unwrap_or("unknown"),
			role,
			worktree.ownership_reason,
			worktree.branch_name,
			worktree.worktree_path,
			worktree.provenance.source,
				orchestrator::format_optional_i64(worktree.provenance.created_at_unix),
				orchestrator::format_optional_i64(worktree.provenance.updated_at_unix),
			worktree.provenance.audit_required,
			worktree.recovery_next_action.as_deref().unwrap_or("none")
		));
	}
}

fn rendered_worktree_role<'a>(
	worktree: &'a OperatorWorktreeStatus,
	snapshot: &'a OperatorStatusSnapshot,
) -> &'a str {
	if !worktree.ownership.trim().is_empty() {
		return worktree.ownership.as_str();
	}
	if snapshot.current_lanes.iter().any(|run| {
		run.ownership_state == "leased_run"
			&& (run.worktree_path.as_deref() == Some(worktree.worktree_path.as_str())
				|| run.branch_name.as_deref() == Some(worktree.branch_name.as_str())
				|| run.issue_id == worktree.issue_id)
	}) {
		return "current_lane";
	}
	if snapshot.post_review_lanes.iter().any(|lane| {
		lane.worktree_path == worktree.worktree_path
			|| lane.branch_name == worktree.branch_name
			|| lane.issue_id == worktree.issue_id
			|| lane.issue_identifier == worktree.issue_id
	}) {
		return "post_review_lane";
	}
	if snapshot.queued_candidates.iter().any(|candidate| {
		matches!(
			candidate.reason.as_str(),
			"issue_needs_attention" | QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT
		) && (candidate.attention.as_ref().and_then(|attention| attention.worktree_path.as_deref())
			== Some(worktree.worktree_path.as_str())
			|| candidate.issue_id == worktree.issue_id
			|| candidate.issue_identifier == worktree.issue_id)
	}) {
		return "blocked_queue_issue";
	}

	"orphaned_local_worktree"
}

fn rendered_worktree_role_rank(role: &str) -> u8 {
	match role {
		"current_lane"
		| "running_lane"
		| "blocked_queue_issue"
		| "queued_attention"
		| "continuation_pending" => 0,
		"post_review_lane" => 1,
		_ => 2,
	}
}
