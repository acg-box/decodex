mod failure;
mod labels;
mod local_state;
mod rollback;

use crate::{
	prelude::Result,
	recovery::{
		self, AdoptValidation, REBOUND_ORCHESTRATION_PHASE, RecoveryContext,
		review_handoff_apply::audit,
	},
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker},
	tracker::IssueTracker,
};

pub(in crate::recovery) fn apply_review_handoff_adopt(
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
	let local_state_write = local_state::write_adopt_local_state(
		context,
		validation,
		&handoff_marker,
		&orchestration_marker,
	);

	if let Err(error) = local_state_write {
		failure::mark_adopt_attempt_failed(context, validation);

		context.state_store.clear_review_lifecycle_for_handoff(
			context.config.service_id(),
			&validation.issue.id,
			&handoff_marker,
			&orchestration_marker,
		)?;

		rollback::rollback_adopt_worktree_mapping(context, validation)?;

		return Err(error);
	}

	let active_label_restored = match labels::restore_adopt_active_label(context, validation) {
		Ok(active_label_restored) => active_label_restored,
		Err(error) => {
			failure::mark_adopt_attempt_failed(context, validation);

			context.state_store.clear_review_lifecycle_for_handoff(
				context.config.service_id(),
				&validation.issue.id,
				&handoff_marker,
				&orchestration_marker,
			)?;

			rollback::rollback_adopt_worktree_mapping(context, validation)?;

			return Err(error);
		},
	};
	let event = recovery::review_handoff_adopt_event(context, validation, active_label_restored);

	if let Err(error) = audit::write_adopt_audit(context, validation, &event)
		.and_then(|()| context.state_store.record_linear_execution_event(&event))
	{
		failure::mark_adopt_attempt_failed(context, validation);

		context.state_store.clear_review_lifecycle_for_handoff(
			context.config.service_id(),
			&validation.issue.id,
			&handoff_marker,
			&orchestration_marker,
		)?;

		labels::rollback_adopt_active_label_restoration(
			context,
			validation,
			active_label_restored,
		)?;
		rollback::rollback_adopt_worktree_mapping(context, validation)?;

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
