use serde::Deserialize;
use serde_json::Value;

use crate::{
	autonomy_proposal::{AutonomyProposalChallengeInput, AutonomyProposalChallengeSource},
	mcp::{
		self, TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
		planning::{self, PlanningAuthorityArgs},
	},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct AutonomyChallengeProposalToolArgs {
	pub(in crate::mcp) mode: Option<String>,
	pub(in crate::mcp) project_id: Option<String>,
	pub(in crate::mcp) proposal_id: String,
	pub(in crate::mcp) challenge: AutonomyProposalChallengeArgs,
	pub(in crate::mcp) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct AutonomyProposalChallengeArgs {
	pub(in crate::mcp) source: AutonomyProposalChallengeSource,
	pub(in crate::mcp) actor: String,
	pub(in crate::mcp) summary: String,
	#[serde(default)]
	pub(in crate::mcp) objections: Vec<String>,
	#[serde(default)]
	pub(in crate::mcp) evidence_refs: Vec<String>,
	pub(in crate::mcp) recorded_at: Option<String>,
}
impl AutonomyProposalChallengeArgs {
	pub(in crate::mcp) fn into_challenge_input(
		self,
	) -> Result<AutonomyProposalChallengeInput, Value> {
		if mcp::non_empty_string(Some(self.actor.as_str())).is_none()
			|| mcp::non_empty_string(Some(self.summary.as_str())).is_none()
		{
			return Err(mcp::invalid_tool_arguments(
				TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
				"`challenge.actor` and `challenge.summary` are required.",
			));
		}

		Ok(AutonomyProposalChallengeInput {
			source: self.source,
			actor: self.actor,
			summary: self.summary,
			objections: self.objections,
			evidence_refs: self.evidence_refs,
			recorded_at: self.recorded_at.unwrap_or_else(planning::mcp_now_rfc3339),
		})
	}
}
