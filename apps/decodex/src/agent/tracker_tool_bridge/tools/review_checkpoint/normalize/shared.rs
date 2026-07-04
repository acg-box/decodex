use crate::agent::tracker_tool_bridge::{self, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME};

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint) fn normalize_review_severity(
	severity: String,
	field_name: &str,
) -> Result<String, String> {
	let severity = severity.trim().to_ascii_lowercase();

	match severity.as_str() {
		"critical" | "high" | "medium" | "low" | "info" => Ok(severity),
		other => Err(format!(
			"`{field_name}` must be `critical`, `high`, `medium`, `low`, or `info`, not `{other}`."
		)),
	}
}

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint) fn normalize_required_review_text(
	value: String,
	field_name: &str,
) -> Result<String, String> {
	let value = tracker_tool_bridge::normalize_summary(&value);

	if value.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(value)
}

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint) fn normalize_required_review_evidence_list(
	values: Vec<String>,
	field_name: &str,
) -> Result<Vec<String>, String> {
	let values = tracker_tool_bridge::normalize_progress_list(values);

	if values.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(values)
}
