use crate::{
	prelude::Result,
	recovery::{
		self, AdoptValidation, ConfiguredPublicProjectionPrivacyClassifier,
		LinearExecutionEventRecord, RebindValidation, RecoveryContext,
	},
	tracker::{self, records},
};

pub(in crate::recovery::review_handoff_apply) fn write_rebind_audit(
	context: &RecoveryContext,
	validation: &RebindValidation,
	event: &LinearExecutionEventRecord,
) -> Result<()> {
	let recovery_body = format!(
		"Decodex operator recovery: {} for `{}` to `{}`. This does not land the pull request.",
		validation.mode.summary_action(),
		validation.issue.identifier,
		recovery::landing_url(&validation.landing_state)
	);
	let retry_budget_attempt_count =
		context.state_store.retry_budget_attempt_count(&validation.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body = format!(
		"{recovery_body}\n\n{}",
		records::render_linear_execution_event_comment_body(event, retry_budget_attempt_count)
	);
	let privacy_classifier = ConfiguredPublicProjectionPrivacyClassifier::from_config(
		context.config.privacy_classifier(),
	)?;
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, event, &privacy_classifier)?;

	tracker::create_prepared_linear_execution_event_comment(
		&context.tracker,
		&validation.issue.id,
		&projection,
	)?;

	Ok(())
}

pub(in crate::recovery::review_handoff_apply) fn write_adopt_audit(
	context: &RecoveryContext,
	validation: &AdoptValidation,
	event: &LinearExecutionEventRecord,
) -> Result<()> {
	let recovery_body = format!(
		"Decodex operator recovery: adopted human-owned PR `{}` for `{}` into retained review handoff state. This does not land the pull request.",
		recovery::landing_url(&validation.landing_state),
		validation.issue.identifier,
	);
	let retry_budget_attempt_count =
		context.state_store.retry_budget_attempt_count(&validation.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body = format!(
		"{recovery_body}\n\n{}",
		records::render_linear_execution_event_comment_body(event, retry_budget_attempt_count)
	);
	let privacy_classifier = ConfiguredPublicProjectionPrivacyClassifier::from_config(
		context.config.privacy_classifier(),
	)?;
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, event, &privacy_classifier)?;

	tracker::create_prepared_linear_execution_event_comment(
		&context.tracker,
		&validation.issue.id,
		&projection,
	)?;

	Ok(())
}
