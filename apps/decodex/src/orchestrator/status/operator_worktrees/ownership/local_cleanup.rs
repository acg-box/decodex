use crate::{orchestrator::OperatorWorktreeStatus, state::WORKTREE_PROVENANCE_LEGACY_UNKNOWN};

pub(crate) fn worktree_cleanup_only_reason(
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

pub(crate) fn legacy_cleanup_next_action(worktree: &OperatorWorktreeStatus) -> String {
	let issue = worktree.issue_identifier.as_deref().unwrap_or(&worktree.issue_id);

	format!(
		"verify tracker/PR terminal state and clean git status for `{}`, then run `decodex recover legacy-closeout {issue} --pr <MERGED_PR> --dry-run`; rerun with `--manual-authority` before removing this worktree",
		worktree.worktree_path
	)
}
