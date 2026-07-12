use crate::{
	lane_authority::{LaneCommand, LaneId},
	orchestrator,
	prelude::Result,
	recovery::{AdoptValidation, RecoveryContext},
};

pub(in crate::recovery::review_handoff_apply::adopt) fn mark_adopt_attempt_failed(
	context: &RecoveryContext,
	validation: &AdoptValidation,
) {
	if let Err(error) = context.state_store.update_run_status(&validation.run_id, "failed") {
		tracing::warn!(
			?error,
			run_id = %validation.run_id,
			"Failed to mark manual takeover adopt attempt failed."
		);
	}
}

pub(in crate::recovery::review_handoff_apply::adopt) fn guard_adopt_lane_after_external_failure(
	context: &RecoveryContext,
	validation: &AdoptValidation,
) -> Result<()> {
	mark_adopt_attempt_failed(context, validation);
	let project_id = context.config.service_id();
	let attestation = orchestrator::attest_issue_project_binding(
		&context.state_store,
		&context.config,
		&validation.issue,
	)?;
	context
		.state_store
		.apply_lane_command(
			LaneId::new(project_id, &validation.issue.id)?,
			attestation.binding_fingerprint(),
			LaneCommand::RequireAttention,
		)
		.map(|_| ())
}
