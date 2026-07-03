//! Review handoff recovery application and audit writes.

use crate::{
	prelude::Result,
	recovery::{
		self, AdoptValidation, ConfiguredPublicProjectionPrivacyClassifier,
		LinearExecutionEventRecord, REBOUND_ORCHESTRATION_PHASE, RebindValidation, RecoveryContext,
	},
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker, StateStore},
	tracker::{self, IssueTracker, records},
};

pub(super) fn apply_review_handoff_rebind(
	context: &RecoveryContext,
	validation: &RebindValidation,
) -> Result<()> {
	let handoff_marker = ReviewHandoffMarker::new(
		validation.run_id.clone(),
		validation.attempt_number,
		validation.worktree.branch_name(),
		recovery::landing_url(&validation.landing_state),
		validation.landing_state.base_ref_name.clone(),
		validation.landing_state.head_ref_name.clone(),
		validation.local_head_oid.clone(),
	);
	let orchestration_marker = ReviewOrchestrationMarker::new(
		validation.run_id.clone(),
		validation.attempt_number,
		validation.worktree.branch_name(),
		recovery::landing_url(&validation.landing_state),
		validation.local_head_oid.clone(),
		REBOUND_ORCHESTRATION_PHASE,
		None,
		None,
		None,
		0,
		0,
		None,
	);

	write_review_lifecycle_markers_with_rollback(
		&context.state_store,
		context.config.service_id(),
		&validation.issue.id,
		&handoff_marker,
		&orchestration_marker,
		|| {
			context.state_store.upsert_review_orchestration_marker(
				context.config.service_id(),
				&validation.issue.id,
				&orchestration_marker,
			)
		},
	)?;

	let active_label_restored = match restore_rebind_active_label(context, validation) {
		Ok(active_label_restored) => active_label_restored,
		Err(error) => {
			context.state_store.clear_review_lifecycle_for_handoff(
				context.config.service_id(),
				&validation.issue.id,
				&handoff_marker,
				&orchestration_marker,
			)?;

			return Err(error);
		},
	};
	let event = recovery::review_handoff_rebind_event(context, validation, active_label_restored);

	if let Err(error) = write_rebind_audit(context, validation, &event)
		.and_then(|()| context.state_store.record_linear_execution_event(&event))
	{
		context.state_store.clear_review_lifecycle_for_handoff(
			context.config.service_id(),
			&validation.issue.id,
			&handoff_marker,
			&orchestration_marker,
		)?;

		rollback_rebind_active_label_restoration(context, validation, active_label_restored)?;

		return Err(error);
	}

	if validation.clear_needs_attention_label {
		tracker::set_issue_label_presence(
			&context.tracker,
			&validation.issue,
			context.workflow.frontmatter().tracker().needs_attention_label(),
			false,
		)?;
	}

	if let Some(transition) = validation.success_state_transition.as_ref() {
		context.tracker.update_issue_state(&validation.issue.id, &transition.state_id)?;
	}

	recovery::append_review_handoff_rebind_private_event(
		&context.state_store,
		context.config.service_id(),
		validation,
		"local_markers_written",
		active_label_restored,
	)?;

	Ok(())
}

pub(super) fn write_review_lifecycle_markers_with_rollback<F>(
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

pub(super) fn apply_review_handoff_adopt(
	context: &RecoveryContext,
	validation: &AdoptValidation,
) -> Result<()> {
	let handoff_marker = ReviewHandoffMarker::new(
		validation.run_id.clone(),
		validation.attempt_number,
		validation.branch_name.clone(),
		recovery::landing_url(&validation.landing_state),
		validation.landing_state.base_ref_name.clone(),
		validation.landing_state.head_ref_name.clone(),
		validation.local_head_oid.clone(),
	);
	let orchestration_marker = ReviewOrchestrationMarker::new(
		validation.run_id.clone(),
		validation.attempt_number,
		validation.branch_name.clone(),
		recovery::landing_url(&validation.landing_state),
		validation.local_head_oid.clone(),
		REBOUND_ORCHESTRATION_PHASE,
		None,
		None,
		None,
		0,
		0,
		None,
	);
	let local_state_write =
		write_adopt_local_state(context, validation, &handoff_marker, &orchestration_marker);

	if let Err(error) = local_state_write {
		mark_adopt_attempt_failed(context, validation);

		context.state_store.clear_review_lifecycle_for_handoff(
			context.config.service_id(),
			&validation.issue.id,
			&handoff_marker,
			&orchestration_marker,
		)?;

		rollback_adopt_worktree_mapping(context, validation)?;

		return Err(error);
	}

	let active_label_restored = match restore_adopt_active_label(context, validation) {
		Ok(active_label_restored) => active_label_restored,
		Err(error) => {
			mark_adopt_attempt_failed(context, validation);

			context.state_store.clear_review_lifecycle_for_handoff(
				context.config.service_id(),
				&validation.issue.id,
				&handoff_marker,
				&orchestration_marker,
			)?;

			rollback_adopt_worktree_mapping(context, validation)?;

			return Err(error);
		},
	};
	let event = recovery::review_handoff_adopt_event(context, validation, active_label_restored);

	if let Err(error) = write_adopt_audit(context, validation, &event)
		.and_then(|()| context.state_store.record_linear_execution_event(&event))
	{
		mark_adopt_attempt_failed(context, validation);

		context.state_store.clear_review_lifecycle_for_handoff(
			context.config.service_id(),
			&validation.issue.id,
			&handoff_marker,
			&orchestration_marker,
		)?;

		rollback_adopt_active_label_restoration(context, validation, active_label_restored)?;
		rollback_adopt_worktree_mapping(context, validation)?;

		return Err(error);
	}
	if let Some(transition) = validation.success_state_transition.as_ref() {
		context.tracker.update_issue_state(&validation.issue.id, &transition.state_id)?;
	}

	recovery::append_review_handoff_adopt_private_event(
		&context.state_store,
		context.config.service_id(),
		validation,
		"active_label_checked",
		active_label_restored,
	)?;

	Ok(())
}

fn write_adopt_local_state(
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

fn restore_rebind_active_label(
	context: &RecoveryContext,
	validation: &RebindValidation,
) -> Result<bool> {
	if !validation.should_restore_active_label() {
		return Ok(false);
	}

	let active_label = tracker::automation_active_label(context.config.service_id());

	tracker::set_issue_label_presence(&context.tracker, &validation.issue, &active_label, true)
}

fn rollback_rebind_active_label_restoration(
	context: &RecoveryContext,
	validation: &RebindValidation,
	active_label_restored: bool,
) -> Result<()> {
	if !active_label_restored {
		return Ok(());
	}

	let active_label = tracker::automation_active_label(context.config.service_id());

	tracker::set_issue_label_presence(&context.tracker, &validation.issue, &active_label, false)?;

	Ok(())
}

fn restore_adopt_active_label(
	context: &RecoveryContext,
	validation: &AdoptValidation,
) -> Result<bool> {
	if !validation.should_restore_active_label() {
		return Ok(false);
	}

	let active_label = tracker::automation_active_label(context.config.service_id());

	tracker::set_issue_label_presence(&context.tracker, &validation.issue, &active_label, true)
}

fn rollback_adopt_active_label_restoration(
	context: &RecoveryContext,
	validation: &AdoptValidation,
	active_label_restored: bool,
) -> Result<()> {
	if !active_label_restored {
		return Ok(());
	}

	let active_label = tracker::automation_active_label(context.config.service_id());

	tracker::set_issue_label_presence(&context.tracker, &validation.issue, &active_label, false)?;

	Ok(())
}

fn rollback_adopt_worktree_mapping(
	context: &RecoveryContext,
	validation: &AdoptValidation,
) -> Result<()> {
	if let Some(mapping) = validation.previous_worktree_mapping.as_ref() {
		let worktree_path = mapping.worktree_path().to_string_lossy();

		return context.state_store.upsert_worktree(
			mapping.project_id(),
			mapping.issue_id(),
			mapping.branch_name(),
			&worktree_path,
		);
	}

	context.state_store.clear_worktree(&validation.issue.id)
}

fn mark_adopt_attempt_failed(context: &RecoveryContext, validation: &AdoptValidation) {
	if let Err(error) = context.state_store.update_run_status(&validation.run_id, "failed") {
		tracing::warn!(
			?error,
			run_id = %validation.run_id,
			"Failed to mark manual takeover adopt attempt failed."
		);
	}
}

fn write_rebind_audit(
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

fn write_adopt_audit(
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
