use serde_json::{self, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::mcp::{self, planning::PlanningAuthorityArgs};

pub(in crate::mcp) fn planning_authority_present(
	authority: Option<&PlanningAuthorityArgs>,
) -> bool {
	let Some(authority) = authority else {
		return false;
	};
	let _lane_preconditions = (
		mcp::non_empty_string(authority.run_id.as_deref()),
		mcp::non_empty_string(authority.expected_turn_id.as_deref()),
	);

	mcp::non_empty_string(authority.source.as_deref()).is_some()
		&& mcp::non_empty_string(authority.reason.as_deref()).is_some()
}

pub(in crate::mcp) fn missing_authority_refusal(tool: &str, message: &str) -> Value {
	mcp::tool_refusal_value(serde_json::json!({
		"schema": "decodex.mcp.refusal/1",
		"status": "refused",
		"reason": "missing_authority",
		"tool": tool,
		"message": message
	}))
}

pub(in crate::mcp) fn mcp_now_rfc3339() -> String {
	OffsetDateTime::now_utc()
		.format(&Rfc3339)
		.unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"))
}
