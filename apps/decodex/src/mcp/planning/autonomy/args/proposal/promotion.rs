use serde::Deserialize;
use serde_json::Value;

use crate::{
	autonomy_proposal::{
		AutonomyProposalAuthorityActorKind, AutonomyProposalDecisionBridgeAuthority,
		AutonomyProposalDecisionBridgeAuthorityInput,
	},
	mcp::{self, planning},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct AutonomyRequestPromotionToolArgs {
	pub(in crate::mcp) mode: Option<String>,
	pub(in crate::mcp) project_id: Option<String>,
	pub(in crate::mcp) proposal_id: String,
	pub(in crate::mcp) authority: Option<AutonomyProposalAcceptanceArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct AutonomyProposalAcceptanceArgs {
	pub(in crate::mcp) accepted_by: String,
	pub(in crate::mcp) accepted_by_kind: AutonomyProposalAuthorityActorKind,
	pub(in crate::mcp) accepted_at: Option<String>,
	pub(in crate::mcp) acceptance_source: String,
	pub(in crate::mcp) reason: String,
	pub(in crate::mcp) proposal_actor: String,
	pub(in crate::mcp) proposal_actor_kind: AutonomyProposalAuthorityActorKind,
	pub(in crate::mcp) accepted_project_policy: Option<Value>,
}
impl AutonomyProposalAcceptanceArgs {
	pub(in crate::mcp) fn into_decision_bridge_authority(
		self,
	) -> Result<AutonomyProposalDecisionBridgeAuthority, Value> {
		if self.accepted_project_policy.is_some() {
			return Err(mcp::tool_refusal(
				"autonomy_policy_authority_refused",
				"acceptedProjectPolicy must be resolved from trusted Decodex authority state; MCP request payloads cannot prove accepted policy authority.",
			));
		}

		AutonomyProposalDecisionBridgeAuthority::new(AutonomyProposalDecisionBridgeAuthorityInput {
			accepted_by: self.accepted_by,
			accepted_by_kind: self.accepted_by_kind,
			accepted_at: self.accepted_at.unwrap_or_else(planning::mcp_now_rfc3339),
			acceptance_source: self.acceptance_source,
			reason: self.reason,
			proposal_actor: self.proposal_actor,
			proposal_actor_kind: self.proposal_actor_kind,
			accepted_project_policy: None,
		})
		.map_err(|error| {
			mcp::tool_refusal(
				"autonomy_acceptance_authority_refused",
				format!("Autonomy proposal acceptance authority was refused: {error}"),
			)
		})
	}
}
