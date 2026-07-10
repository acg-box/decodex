use serde::Deserialize;

use crate::autonomy_proposal::AutonomyProposalAuthorityActorKind;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct AutonomyAcceptRuntimePolicyToolArgs {
	pub(in crate::mcp) mode: Option<String>,
	pub(in crate::mcp) project_id: Option<String>,
	#[serde(default)]
	pub(in crate::mcp) public_non_goals: Vec<String>,
	pub(in crate::mcp) authority: Option<RuntimePolicyAcceptanceAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct RuntimePolicyAcceptanceAuthorityArgs {
	pub(in crate::mcp) accepted_by: String,
	pub(in crate::mcp) accepted_by_kind: AutonomyProposalAuthorityActorKind,
	pub(in crate::mcp) accepted_at: String,
	pub(in crate::mcp) acceptance_source: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct AutonomyApplyRuntimePolicyToolArgs {
	pub(in crate::mcp) mode: Option<String>,
	pub(in crate::mcp) project_id: Option<String>,
	pub(in crate::mcp) proposal_id: String,
}
