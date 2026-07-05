use crate::tracker::{public_text, records::LinearExecutionEventRecord};

pub(in crate::tracker::records::validation) fn validate_linear_execution_event_public_text(
	record: &LinearExecutionEventRecord,
) -> Result<(), String> {
	for (field_name, value) in [
		("summary", record.summary.as_deref()),
		("focus", record.focus.as_deref()),
		("next_action", record.next_action.as_deref()),
		("failed_command", record.failed_command.as_deref()),
		("raw_error", record.raw_error.as_deref()),
	] {
		if let Some(value) = value {
			public_text::validate_public_text_field(field_name, value)?;
		}
	}
	for (field_name, values) in [
		("blockers", record.blockers.as_ref()),
		("evidence", record.evidence.as_ref()),
		("verification", record.verification.as_ref()),
	] {
		if let Some(values) = values {
			public_text::validate_public_text_items(field_name, values)?;
		}
	}

	Ok(())
}
