#[cfg(test)]
use crate::state::{ReviewLifecycleHandoffFixture, ReviewLifecycleTransitionFixture};
use crate::{
	prelude::Result,
	state::{StateStore, runtime_records::ReviewLifecycleKey},
};

impl StateStore {
	pub(crate) fn clear_review_lifecycle_for_identity(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<()> {
		let lifecycle_key = ReviewLifecycleKey::new(project_id, issue_id, branch_name);
		let mut state = self.lock()?;

		if state.review_lifecycle_records.get(&lifecycle_key).is_some_and(|record| {
			record.branch_name == branch_name
				&& record.run_id == run_id
				&& record.attempt_number == attempt_number
		}) {
			state.review_lifecycle_records.remove(&lifecycle_key);
		}

		state.review_policy_checkpoints.retain(|key, _record| {
			key.project_id != project_id
				|| key.issue_id != issue_id
				|| key.run_id != run_id
				|| key.attempt_number != attempt_number
		});
		self.persist_runtime_state_locked(&state)?;

		self.delete_review_marker_identity_locked(
			project_id,
			issue_id,
			branch_name,
			run_id,
			attempt_number,
		)
	}

	pub(crate) fn clear_review_lifecycle_for_issue_run(
		&self,
		issue_id: &str,
		run_id: &str,
	) -> Result<()> {
		let mut state = self.lock()?;

		state
			.review_lifecycle_records
			.retain(|_key, record| record.issue_id != issue_id || record.run_id != run_id);
		state
			.review_policy_checkpoints
			.retain(|key, _record| key.issue_id != issue_id || key.run_id != run_id);

		self.persist_runtime_state_locked(&state)
	}

	/// Remove the exact review lifecycle record created for one handoff identity.
	#[cfg(test)]
	pub(crate) fn clear_review_lifecycle_for_handoff(
		&self,
		project_id: &str,
		issue_id: &str,
		handoff_fixture: &ReviewLifecycleHandoffFixture,
		transition_fixture: &ReviewLifecycleTransitionFixture,
	) -> Result<()> {
		let lifecycle_key =
			ReviewLifecycleKey::new(project_id, issue_id, handoff_fixture.branch_name());
		let mut state = self.lock()?;

		if state
			.review_lifecycle_records
			.get(&lifecycle_key)
			.is_some_and(|record| record.matches_handoff_identity(handoff_fixture))
		{
			state.review_lifecycle_records.remove(&lifecycle_key);
		}

		state.review_policy_checkpoints.retain(|key, _record| {
			key.project_id != project_id
				|| key.issue_id != issue_id
				|| key.run_id != transition_fixture.run_id()
				|| key.attempt_number != transition_fixture.attempt_number()
		});
		self.persist_runtime_state_locked(&state)?;

		self.delete_review_marker_identity_locked(
			project_id,
			issue_id,
			handoff_fixture.branch_name(),
			transition_fixture.run_id(),
			transition_fixture.attempt_number(),
		)
	}
}
