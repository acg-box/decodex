//! Worktree ownership and hygiene projection for operator status snapshots.

mod cleanup_debt;
mod discovery;
mod ownership;
mod provenance;

pub(crate) use self::{
	cleanup_debt::ensure_project_has_no_merged_worktree_cleanup_debt,
	discovery::{
		active_shared_issue_ids, operator_status_worktrees, stale_terminal_local_issue_ids,
	},
	ownership::refresh_worktree_ownership,
};
