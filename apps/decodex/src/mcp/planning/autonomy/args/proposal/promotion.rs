use serde::Deserialize;
use serde_json::Value;

use crate::{
	autonomy_proposal::{
		AutonomyProposalAuthorityActorKind, AutonomyProposalDecisionBridgeAuthority,
	},
	mcp::{self, planning},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp::planning::autonomy) struct AutonomyRequestPromotionToolArgs {
	pub(in crate::mcp::planning::autonomy) mode: Option<String>,
	pub(in crate::mcp::planning::autonomy) project_id: Option<String>,
	pub(in crate::mcp::planning::autonomy) proposal_id: String,
	pub(in crate::mcp::planning::autonomy) authority: Option<AutonomyProposalAcceptanceArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp::planning::autonomy) struct AutonomyProposalAcceptanceArgs {
	pub(in crate::mcp::planning::autonomy) accepted_by: String,
	pub(in crate::mcp::planning::autonomy) accepted_by_kind: AutonomyProposalAuthorityActorKind,
	pub(in crate::mcp::planning::autonomy) accepted_at: Option<String>,
	pub(in crate::mcp::planning::autonomy) acceptance_source: String,
	pub(in crate::mcp::planning::autonomy) reason: String,
	pub(in crate::mcp::planning::autonomy) proposal_actor: String,
	pub(in crate::mcp::planning::autonomy) proposal_actor_kind: AutonomyProposalAuthorityActorKind,
	pub(in crate::mcp::planning::autonomy) accepted_project_policy: Option<Value>,
}
impl AutonomyProposalAcceptanceArgs {
	pub(in crate::mcp::planning::autonomy) fn into_decision_bridge_authority(
		self,
	) -> Result<AutonomyProposalDecisionBridgeAuthority, Value> {
		if self.accepted_project_policy.is_some() {
			return Err(mcp::tool_refusal(
				"autonomy_policy_authority_refused",
				"acceptedProjectPolicy must be resolved from trusted Decodex authority state; MCP request payloads cannot prove accepted policy authority.",
			));
		}

		AutonomyProposalDecisionBridgeAuthority::new(
			self.accepted_by,
			self.accepted_by_kind,
			self.accepted_at.unwrap_or_else(planning::mcp_now_rfc3339),
			self.acceptance_source,
			self.reason,
			self.proposal_actor,
			self.proposal_actor_kind,
			None,
		)
		.map_err(|error| {
			mcp::tool_refusal(
				"autonomy_acceptance_authority_refused",
				format!("Autonomy proposal acceptance authority was refused: {error}"),
			)
		})
	}
}
