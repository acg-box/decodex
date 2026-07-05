use crate::{
	prelude::Result,
	recovery::{AdoptValidation, RecoveryContext},
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker},
};

pub(in crate::recovery::review_handoff_apply::adopt) fn write_adopt_local_state(
	context: &RecoveryContext,
	validation: &AdoptValidation,
	handoff_marker: &ReviewHandoffMarker,
	orchestration_marker: &ReviewOrchestrationMarker,
) -> Result<()> {
	let worktree_path = validation.worktree_path.to_string_lossy().to_string();

	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			&validation.issue.id,
			&validation.branch_name,
			&worktree_path,
		)
		.and_then(|()| {
			context.state_store.record_run_attempt(
				&validation.run_id,
				&validation.issue.id,
				validation.attempt_number,
				"starting",
			)
		})
		.and_then(|()| {
			context.state_store.upsert_review_handoff_marker(
				context.config.service_id(),
				&validation.issue.id,
				handoff_marker,
			)
		})
		.and_then(|()| {
			context.state_store.upsert_review_orchestration_marker(
				context.config.service_id(),
				&validation.issue.id,
				orchestration_marker,
			)
		})
}
