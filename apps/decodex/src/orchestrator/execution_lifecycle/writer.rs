use crate::{
	orchestrator::{
		IssueTracker, PublicProjectionPrivacyClassifier, Result, StateStore,
		records::{self, LinearExecutionEventRecord},
	},
	tracker,
};

pub(crate) fn write_lifecycle_event<T>(
	tracker: &T,
	state_store: &StateStore,
	issue_id: &str,
	record: &LinearExecutionEventRecord,
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let retry_budget_attempt_count = state_store.retry_budget_attempt_count(issue_id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body =
		records::render_linear_execution_event_comment_body(record, retry_budget_attempt_count);
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, record, privacy_classifier)?;

	if state_store.record_linear_execution_event(&projection.record)?
		&& let Err(error) =
			tracker::create_prepared_linear_execution_event_comment_without_remote_scan(
				tracker,
				issue_id,
				&projection,
			) {
		state_store.forget_linear_execution_event(&projection.record.idempotency_key)?;

		return Err(error);
	}

	Ok(())
}
