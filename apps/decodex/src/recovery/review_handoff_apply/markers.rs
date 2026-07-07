#[cfg(test)]
use crate::state::{ReviewHandoffMarker, ReviewOrchestrationMarker};
use crate::{
	prelude::Result,
	state::{ReviewLifecycleHandoffInput, ReviewLifecycleTransitionInput, StateStore},
};

pub(in crate::recovery) fn write_review_lifecycle_with_rollback(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	handoff_input: ReviewLifecycleHandoffInput<'_>,
	transition_input: ReviewLifecycleTransitionInput<'_>,
) -> Result<()> {
	if let Err(error) = state_store
		.record_review_lifecycle_handoff(project_id, issue_id, handoff_input)
		.and_then(|()| {
			state_store.record_review_lifecycle_transition(project_id, issue_id, transition_input)
		}) {
		state_store.clear_review_lifecycle_for_identity(
			project_id,
			issue_id,
			handoff_input.branch_name,
			handoff_input.run_id,
			handoff_input.attempt_number,
		)?;

		return Err(error);
	}

	Ok(())
}

#[cfg(test)]
pub(in crate::recovery) fn write_review_lifecycle_markers_with_rollback<F>(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	handoff_marker: &ReviewHandoffMarker,
	orchestration_marker: &ReviewOrchestrationMarker,
	write_orchestration_marker: F,
) -> Result<()>
where
	F: FnOnce() -> Result<()>,
{
	if let Err(error) = state_store
		.upsert_review_handoff_marker(project_id, issue_id, handoff_marker)
		.and_then(|()| write_orchestration_marker())
	{
		state_store.clear_review_lifecycle_for_handoff(
			project_id,
			issue_id,
			handoff_marker,
			orchestration_marker,
		)?;

		return Err(error);
	}

	Ok(())
}
