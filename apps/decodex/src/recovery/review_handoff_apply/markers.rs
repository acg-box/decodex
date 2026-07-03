use crate::{
	prelude::Result,
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker, StateStore},
};

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
