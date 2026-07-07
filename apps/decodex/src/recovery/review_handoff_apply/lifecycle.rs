#[cfg(test)]
use crate::state::{ReviewLifecycleHandoffFixture, ReviewLifecycleTransitionFixture};
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
pub(in crate::recovery) fn write_review_lifecycle_fixtures_with_rollback<F>(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	handoff_fixture: &ReviewLifecycleHandoffFixture,
	transition_fixture: &ReviewLifecycleTransitionFixture,
	write_transition_fixture: F,
) -> Result<()>
where
	F: FnOnce() -> Result<()>,
{
	if let Err(error) = state_store
		.upsert_review_lifecycle_handoff_fixture(project_id, issue_id, handoff_fixture)
		.and_then(|()| write_transition_fixture())
	{
		state_store.clear_review_lifecycle_for_handoff(
			project_id,
			issue_id,
			handoff_fixture,
			transition_fixture,
		)?;

		return Err(error);
	}

	Ok(())
}
