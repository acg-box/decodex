use crate::tracker::records::{
	LINEAR_EXECUTION_EVENT_RECORD_TYPE, LINEAR_EXECUTION_EVENT_RECORD_VERSION,
	LinearExecutionEventRecord,
};

pub(in crate::tracker::records::validation) fn validate_linear_execution_event_envelope(
	record: &LinearExecutionEventRecord,
) -> Result<(), String> {
	if record.record_type != LINEAR_EXECUTION_EVENT_RECORD_TYPE {
		return Err(format!("`record_type` must be `{LINEAR_EXECUTION_EVENT_RECORD_TYPE}`."));
	}
	if record.record_version != LINEAR_EXECUTION_EVENT_RECORD_VERSION {
		return Err(format!("`record_version` must be `{LINEAR_EXECUTION_EVENT_RECORD_VERSION}`."));
	}

	for (field, value) in [
		("event_type", record.event_type.as_str()),
		("event_timestamp", record.event_timestamp.as_str()),
		("idempotency_key", record.idempotency_key.as_str()),
		("service_id", record.service_id.as_str()),
		("issue_id", record.issue_id.as_str()),
		("issue_identifier", record.issue_identifier.as_str()),
		("run_id", record.run_id.as_str()),
	] {
		if value.trim().is_empty() {
			return Err(format!("`{field}` must not be empty."));
		}
	}

	if record.attempt_number < 1 {
		return Err(String::from("`attempt_number` must be at least 1."));
	}

	Ok(())
}
