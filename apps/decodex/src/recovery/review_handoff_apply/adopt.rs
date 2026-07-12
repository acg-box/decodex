mod effects;
mod failure;
mod labels;
mod local_state;
mod rollback;

use crate::{
	prelude::Result,
	recovery::{
		self, AdoptValidation, REBOUND_LIFECYCLE_PHASE, RecoveryContext,
		review_handoff_apply::audit,
	},
	state::{ReviewLifecycleHandoffInput, ReviewLifecycleTransitionInput},
	tracker::IssueTracker,
};

pub(in crate::recovery) fn apply_review_handoff_adopt(
	context: &RecoveryContext,
	validation: &AdoptValidation,
) -> Result<()> {
	let handoff_input = ReviewLifecycleHandoffInput {
		run_id: &validation.run_id,
		attempt_number: validation.attempt_number,
		branch_name: &validation.branch_name,
		pr_url: recovery::landing_url(&validation.landing_state),
		base_ref_name: &validation.landing_state.base_ref_name,
		head_ref_name: &validation.landing_state.head_ref_name,
		head_sha: &validation.local_head_oid,
	};
	let local_state_write = local_state::write_adopt_local_state(
		context,
		validation,
		handoff_input,
		ReviewLifecycleTransitionInput {
			run_id: &validation.run_id,
			attempt_number: validation.attempt_number,
			branch_name: &validation.branch_name,
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
	);

	if let Err(error) = local_state_write {
		failure::mark_adopt_attempt_failed(context, validation);

		context.state_store.clear_review_lifecycle_for_identity(
			context.config.service_id(),
			&validation.issue.id,
			handoff_input.branch_name,
			handoff_input.run_id,
			handoff_input.attempt_number,
		)?;

		rollback::rollback_adopt_worktree_mapping(context, validation)?;

		return Err(error);
	}

	let active_label_restored = match labels::restore_adopt_active_label(context, validation) {
		Ok(active_label_restored) => active_label_restored,
		Err(error) => {
			failure::guard_adopt_lane_after_external_failure(context, validation)?;
			return Err(error);
		},
	};
	let event = recovery::review_handoff_adopt_event(context, validation, active_label_restored);

	let audit_effect = effects::plan_adopt_audit_effect(context, validation, &event)?;
	let invoking = effects::begin_adopt_audit_effect(context, &audit_effect)?;
	let audit_created = if invoking.state() == crate::lane_authority::EffectState::Succeeded {
		false
	} else {
		match audit::write_adopt_audit(context, validation, &event) {
			Ok(created) => created,
			Err(error) => {
				effects::mark_adopt_audit_outcome_unknown(context, &invoking)?;
				return Err(error);
			},
		}
	};
	if invoking.state() != crate::lane_authority::EffectState::Succeeded {
		effects::record_adopt_audit_receipt(context, &invoking, audit_created)?;
	}
	if let Err(error) = context.state_store.record_linear_execution_event(&event) {
		failure::guard_adopt_lane_after_external_failure(context, validation)?;
		return Err(error);
	}
	if let Some(transition) = validation.success_state_transition.as_ref() {
		if let Err(error) =
			context.tracker.update_issue_state(&validation.issue.id, &transition.state_id)
		{
			failure::guard_adopt_lane_after_external_failure(context, validation)?;
			return Err(error);
		}
	}

	if let Err(error) = recovery::append_review_handoff_adopt_private_event(
		&context.state_store,
		context.config.service_id(),
		validation,
		"active_label_checked",
		active_label_restored,
	) {
		failure::guard_adopt_lane_after_external_failure(context, validation)?;
		return Err(error);
	}

	Ok(())
}
