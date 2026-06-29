use serde_json::{self, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::mcp::{TOOL_RESEARCH_PROMOTE, non_empty_string, tool_refusal_value};

use super::PlanningAuthorityArgs;

pub(super) struct PromotionAuthority<'a> {
	pub(super) accepted_by: &'a str,
	pub(super) accepted_at: Option<&'a String>,
	pub(super) acceptance_source: &'a str,
	pub(super) reason: Option<&'a String>,
}

pub(super) fn planning_authority_present(authority: Option<&PlanningAuthorityArgs>) -> bool {
	let Some(authority) = authority else {
		return false;
	};
	let _lane_preconditions = (
		non_empty_string(authority.run_id.as_deref()),
		non_empty_string(authority.expected_turn_id.as_deref()),
	);

	non_empty_string(authority.source.as_deref()).is_some()
		&& non_empty_string(authority.reason.as_deref()).is_some()
}

pub(super) fn promotion_authority(
	authority: Option<&PlanningAuthorityArgs>,
) -> Result<PromotionAuthority<'_>, Value> {
	let Some(authority) = authority else {
		return Err(missing_authority_refusal(
			TOOL_RESEARCH_PROMOTE,
			"research_promote apply requires authority.acceptedBy and authority.acceptanceSource.",
		));
	};
	let accepted_by = non_empty_string(authority.accepted_by.as_deref()).ok_or_else(|| {
		missing_authority_refusal(
			TOOL_RESEARCH_PROMOTE,
			"research_promote apply requires authority.acceptedBy.",
		)
	})?;
	let acceptance_source =
		non_empty_string(authority.acceptance_source.as_deref()).ok_or_else(|| {
			missing_authority_refusal(
				TOOL_RESEARCH_PROMOTE,
				"research_promote apply requires authority.acceptanceSource.",
			)
		})?;

	Ok(PromotionAuthority {
		accepted_by,
		accepted_at: authority.accepted_at.as_ref(),
		acceptance_source,
		reason: authority.reason.as_ref(),
	})
}

pub(super) fn missing_authority_refusal(tool: &str, message: &str) -> Value {
	tool_refusal_value(serde_json::json!({
		"schema": "decodex.mcp.refusal/1",
		"status": "refused",
		"reason": "missing_authority",
		"tool": tool,
		"message": message
	}))
}

pub(super) fn mcp_now_rfc3339() -> String {
	OffsetDateTime::now_utc()
		.format(&Rfc3339)
		.unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"))
}
