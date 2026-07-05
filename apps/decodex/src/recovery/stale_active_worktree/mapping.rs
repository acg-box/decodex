use crate::{
	prelude::{Result, eyre},
	state::{StateStore, WorktreeMapping},
};

pub(in crate::recovery) fn stale_active_worktree_mapping_for_keys(
	state_store: &StateStore,
	issue_keys: &[String],
) -> Result<Option<WorktreeMapping>> {
	let mut mapping = None;

	for issue_key in issue_keys {
		let Some(candidate) = state_store.worktree_for_issue(issue_key)? else {
			continue;
		};

		if let Some(existing) = mapping.as_ref() {
			if stale_active_worktree_mappings_conflict(existing, &candidate) {
				eyre::bail!(
					"conflicting retained worktree mappings for stale active issue keys `{}`",
					issue_keys.join(", ")
				);
			}
		} else {
			mapping = Some(candidate);
		}
	}

	Ok(mapping)
}

fn stale_active_worktree_mappings_conflict(
	left: &WorktreeMapping,
	right: &WorktreeMapping,
) -> bool {
	left.branch_name() != right.branch_name() || left.worktree_path() != right.worktree_path()
}
