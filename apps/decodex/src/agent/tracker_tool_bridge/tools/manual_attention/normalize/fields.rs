use crate::agent::tracker_tool_bridge::{
	self, ISSUE_COMMENT_TOOL_NAME, tools::COMMENT_KIND_MANUAL_ATTENTION,
};

pub(in crate::agent::tracker_tool_bridge::tools::manual_attention::normalize) fn normalize_required_comment_field(
	value: Option<String>,
	field_name: &str,
) -> Result<String, String> {
	let value = tracker_tool_bridge::normalize_optional_progress_field(value).ok_or_else(|| {
		format!(
			"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires `{field_name}`."
		)
	})?;

	Ok(value)
}

pub(in crate::agent::tracker_tool_bridge::tools::manual_attention::normalize) fn normalize_required_decision_request_field(
	value: Option<String>,
	field_name: &str,
) -> Result<String, String> {
	tracker_tool_bridge::normalize_optional_progress_field(value)
		.ok_or_else(|| format!("`decision_request.{field_name}` must be present and non-empty."))
}
