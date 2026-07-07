use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct IntakeGoalToolArgs {
	pub(in crate::mcp) mode: Option<String>,
	pub(in crate::mcp) contract_id: String,
	pub(in crate::mcp) team_issue_identifier: Option<String>,
	pub(in crate::mcp) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct PlanningAuthorityArgs {
	pub(in crate::mcp) source: Option<String>,
	pub(in crate::mcp) reason: Option<String>,
	pub(in crate::mcp) run_id: Option<String>,
	pub(in crate::mcp) expected_turn_id: Option<String>,
}
