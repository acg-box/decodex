use serde::Deserialize;

use crate::mcp;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LaneControlToolArgs {
	pub(super) action: String,
	pub(super) project_id: Option<String>,
	pub(super) issue: Option<String>,
	pub(super) run_id: Option<String>,
	pub(super) expected_turn_id: Option<String>,
	pub(super) message: Option<String>,
	pub(super) force: Option<bool>,
	pub(super) authority: Option<LaneControlAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LaneControlAuthorityArgs {
	pub(super) reason: Option<String>,
	pub(super) source: Option<String>,
	pub(super) inspected_run_id: Option<String>,
	pub(super) expected_turn_id: Option<String>,
	pub(super) allow_hard_fallback: Option<bool>,
}

pub(super) struct LaneControlAuthority<'a> {
	pub(super) reason: &'a str,
	pub(super) source: &'a str,
	pub(super) inspected_run_id: &'a str,
	pub(super) expected_turn_id: Option<&'a str>,
	pub(super) allow_hard_fallback: bool,
}

pub(super) fn lane_control_authority(
	params: &LaneControlToolArgs,
) -> Option<LaneControlAuthority<'_>> {
	let authority = params.authority.as_ref()?;

	Some(LaneControlAuthority {
		reason: mcp::non_empty_string(authority.reason.as_deref())?,
		source: mcp::non_empty_string(authority.source.as_deref())?,
		inspected_run_id: mcp::non_empty_string(authority.inspected_run_id.as_deref())?,
		expected_turn_id: mcp::non_empty_string(authority.expected_turn_id.as_deref()),
		allow_hard_fallback: authority.allow_hard_fallback.unwrap_or(false),
	})
}
