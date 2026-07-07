use crate::{
	prelude::Result,
	recovery::{
		self, REBOUND_LIFECYCLE_PHASE, RebindValidation, RecoveryContext,
		review_handoff_apply::{audit, lifecycle},
	},
	state::{ReviewLifecycleHandoffInput, ReviewLifecycleTransitionInput},
	tracker::{self, IssueTracker},
};

pub(in crate::recovery) fn apply_review_handoff_rebind(
	context: &RecoveryContext,
	validation: &RebindValidation,
) -> Result<()> {
	let handoff_input = ReviewLifecycleHandoffInput {
		run_id: &validation.run_id,
		attempt_number: validation.attempt_number,
		branch_name: validation.worktree.branch_name(),
		pr_url: recovery::landing_url(&validation.landing_state),
		base_ref_name: &validation.landing_state.base_ref_name,
		head_ref_name: &validation.landing_state.head_ref_name,
		head_sha: &validation.local_head_oid,
	};
	lifecycle::write_review_lifecycle_with_rollback(
		&context.state_store,
		context.config.service_id(),
		&validation.issue.id,
		handoff_input,
		ReviewLifecycleTransitionInput {
			run_id: &validation.run_id,
			attempt_number: validation.attempt_number,
			branch_name: validation.worktree.branch_name(),
			pr_url: recovery::landing_url(&validation.landing_state),
			head_sha: &validation.local_head_oid,
			phase: REBOUND_LIFECYCLE_PHASE,
			request_comment_database_id: None,
			request_created_at_unix_epoch: None,
			request_description_thumbs_up_count: None,
			request_retry_count: 0,
			external_round_count: 0,
			auto_merge_enabled_at_unix_epoch: None,
		},
	)?;

	let active_label_restored = match restore_rebind_active_label(context, validation) {
		Ok(active_label_restored) => active_label_restored,
		Err(error) => {
			context.state_store.clear_review_lifecycle_for_identity(
				context.config.service_id(),
				&validation.issue.id,
				handoff_input.branch_name,
				handoff_input.run_id,
				handoff_input.attempt_number,
			)?;

			return Err(error);
		},
	};
	let event = recovery::review_handoff_rebind_event(context, validation, active_label_restored);

	if let Err(error) = audit::write_rebind_audit(context, validation, &event)
		.and_then(|()| context.state_store.record_linear_execution_event(&event))
	{
		context.state_store.clear_review_lifecycle_for_identity(
			context.config.service_id(),
			&validation.issue.id,
			handoff_input.branch_name,
			handoff_input.run_id,
			handoff_input.attempt_number,
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
		"local_lifecycle_written",
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
