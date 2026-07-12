use crate::{
	prelude::Result,
	recovery::{AdoptValidation, RecoveryContext},
	tracker,
};

pub(in crate::recovery::review_handoff_apply::adopt) fn restore_adopt_active_label(
	context: &RecoveryContext,
	validation: &AdoptValidation,
) -> Result<bool> {
	if !validation.should_restore_active_label() {
		return Ok(false);
	}

	let active_label = tracker::automation_active_label(context.config.service_id());

	tracker::set_issue_label_presence(&context.tracker, &validation.issue, &active_label, true)
}
