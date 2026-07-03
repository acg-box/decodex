use crate::{
	prelude::Result,
	recovery::{
		self, REBOUND_ORCHESTRATION_PHASE, RebindValidation, RecoveryContext,
		review_handoff_apply::{audit, markers},
	},
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker},
	tracker::{self, IssueTracker},
};

pub(in crate::recovery) fn apply_review_handoff_rebind(
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

	markers::write_review_lifecycle_markers_with_rollback(
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

	if let Err(error) = audit::write_rebind_audit(context, validation, &event)
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
