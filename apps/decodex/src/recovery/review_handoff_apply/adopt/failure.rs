use crate::recovery::{AdoptValidation, RecoveryContext};

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
