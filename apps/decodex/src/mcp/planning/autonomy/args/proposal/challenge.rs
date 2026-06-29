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
pub(in crate::mcp::planning::autonomy) struct AutonomyChallengeProposalToolArgs {
	pub(in crate::mcp::planning::autonomy) mode: Option<String>,
	pub(in crate::mcp::planning::autonomy) project_id: Option<String>,
	pub(in crate::mcp::planning::autonomy) proposal_id: String,
	pub(in crate::mcp::planning::autonomy) challenge: AutonomyProposalChallengeArgs,
	pub(in crate::mcp::planning::autonomy) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp::planning::autonomy) struct AutonomyProposalChallengeArgs {
	pub(in crate::mcp::planning::autonomy) source: AutonomyProposalChallengeSource,
	pub(in crate::mcp::planning::autonomy) actor: String,
	pub(in crate::mcp::planning::autonomy) summary: String,
	#[serde(default)]
	pub(in crate::mcp::planning::autonomy) objections: Vec<String>,
	#[serde(default)]
	pub(in crate::mcp::planning::autonomy) evidence_refs: Vec<String>,
	pub(in crate::mcp::planning::autonomy) recorded_at: Option<String>,
}
impl AutonomyProposalChallengeArgs {
	pub(in crate::mcp::planning::autonomy) fn into_challenge_input(
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
