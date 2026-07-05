use crate::{
	prelude::Result,
	recovery::{AdoptValidation, RecoveryContext},
};

pub(in crate::recovery::review_handoff_apply::adopt) fn rollback_adopt_worktree_mapping(
	context: &RecoveryContext,
	validation: &AdoptValidation,
) -> Result<()> {
	if let Some(mapping) = validation.previous_worktree_mapping.as_ref() {
		let worktree_path = mapping.worktree_path().to_string_lossy();

		return context.state_store.upsert_worktree(
			mapping.project_id(),
			mapping.issue_id(),
			mapping.branch_name(),
			&worktree_path,
		);
	}

	context.state_store.clear_worktree(&validation.issue.id)
}
