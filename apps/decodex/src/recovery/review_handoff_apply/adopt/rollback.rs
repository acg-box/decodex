use crate::{
	lane_authority::{LaneId, LanePhase},
	prelude::Result,
	recovery::{AdoptValidation, RecoveryContext},
};

pub(in crate::recovery::review_handoff_apply::adopt) fn rollback_adopt_worktree_mapping(
	context: &RecoveryContext,
	validation: &AdoptValidation,
) -> Result<()> {
	let project_id = context.config.service_id();
	let issue_id = &validation.issue.id;
	let adopted_path = validation.worktree_path.to_string_lossy();
	let lane_id = LaneId::new(project_id, issue_id)?;
	if let Some(lane) = context.state_store.lane(&lane_id)?
		&& matches!(lane.phase(), LanePhase::Claimed | LanePhase::Running)
		&& lane.branch_name() == Some(validation.branch_name.as_str())
		&& lane.worktree_path().map(|path| path.as_path())
			== Some(validation.worktree_path.as_path())
	{
		context.state_store.detach_claimed_worktree(
			project_id,
			issue_id,
			&validation.branch_name,
			&adopted_path,
		)?;
	}

	if let Some(mapping) = validation.previous_worktree_mapping.as_ref() {
		let worktree_path = mapping.worktree_path().to_string_lossy();
		context.state_store.upsert_claimed_worktree(
			mapping.project_id(),
			mapping.issue_id(),
			mapping.branch_name(),
			&worktree_path,
		)?;
	}
	context.state_store.clear_lease(issue_id)
}
