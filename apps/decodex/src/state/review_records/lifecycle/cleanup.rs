use crate::{
	prelude::Result,
	state::{
		ReviewHandoffMarker, ReviewOrchestrationMarker, StateStore,
		runtime_records::ReviewLifecycleKey,
	},
};

impl StateStore {
	/// Remove the exact review lifecycle record created for one handoff identity.
	pub(crate) fn clear_review_lifecycle_for_handoff(
		&self,
		project_id: &str,
		issue_id: &str,
		handoff_marker: &ReviewHandoffMarker,
		orchestration_marker: &ReviewOrchestrationMarker,
	) -> Result<()> {
		let lifecycle_key =
			ReviewLifecycleKey::new(project_id, issue_id, handoff_marker.branch_name());
		let mut state = self.lock()?;

		if state
			.review_lifecycle_records
			.get(&lifecycle_key)
			.is_some_and(|record| record.matches_handoff_identity(handoff_marker))
		{
			state.review_lifecycle_records.remove(&lifecycle_key);
		}

		state.review_policy_checkpoints.retain(|key, _record| {
			key.project_id != project_id
				|| key.issue_id != issue_id
				|| key.run_id != orchestration_marker.run_id()
				|| key.attempt_number != orchestration_marker.attempt_number()
		});
		self.persist_runtime_state_locked(&state)?;

		self.delete_review_marker_identity_locked(
			project_id,
			issue_id,
			handoff_marker.branch_name(),
			orchestration_marker.run_id(),
			orchestration_marker.attempt_number(),
		)
	}
}
