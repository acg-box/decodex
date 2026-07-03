use crate::{
	prelude::{Result, eyre},
	tracker::{
		privacy_classifier::PublicProjectionPrivacyClassifier,
		public_text, records,
		records::{LinearExecutionEventPublicProjection, LinearExecutionEventRecord},
		types::IssueTracker,
	},
};

pub(crate) fn prepare_linear_execution_event_comment(
	body: &str,
	record: &LinearExecutionEventRecord,
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
) -> Result<LinearExecutionEventPublicProjection> {
	let projection =
		records::linear_execution_event_public_projection(body, record, privacy_classifier);

	records::validate_linear_execution_event_record(&projection.record)
		.map_err(|error| eyre::eyre!(error))?;
	public_text::validate_public_comment_body(&projection.body)
		.map_err(|error| eyre::eyre!(error))?;

	Ok(projection)
}

pub(crate) fn create_prepared_linear_execution_event_comment<T>(
	tracker: &T,
	issue_id: &str,
	projection: &LinearExecutionEventPublicProjection,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let comments = tracker.list_comments(issue_id)?;

	if records::has_linear_execution_event_record(
		&comments,
		&projection.record.service_id,
		&projection.record.issue_id,
		&projection.record.idempotency_key,
	) {
		return Ok(false);
	}

	log_privacy_classifier_withheld_projection(projection);

	let comment_body =
		records::append_structured_comment_record(&projection.body, &projection.record)?;

	tracker.create_comment(issue_id, &comment_body)?;

	Ok(true)
}

pub(crate) fn create_public_comment<T>(tracker: &T, issue_id: &str, body: &str) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	public_text::validate_public_comment_body(body).map_err(|error| eyre::eyre!(error))?;

	tracker.create_comment(issue_id, body)
}

pub(crate) fn create_prepared_linear_execution_event_comment_without_remote_scan<T>(
	tracker: &T,
	issue_id: &str,
	projection: &LinearExecutionEventPublicProjection,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	log_privacy_classifier_withheld_projection(projection);

	let comment_body =
		records::append_structured_comment_record(&projection.body, &projection.record)?;

	tracker.create_comment(issue_id, &comment_body)
}

fn log_privacy_classifier_withheld_projection(projection: &LinearExecutionEventPublicProjection) {
	if projection.classifier_withheld_text {
		tracing::warn!(
			service_id = projection.record.service_id,
			issue_id = projection.record.issue_id,
			event_type = projection.record.event_type,
			"Local privacy classifier withheld Linear public projection text."
		);
	}
}
