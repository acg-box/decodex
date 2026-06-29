use serde::Deserialize;
use serde_json::Value;

use crate::{
	autonomy_proposal::{
		AutonomyProposalAuthorityActorKind, AutonomyProposalChallengeInput,
		AutonomyProposalChallengeSource, AutonomyProposalCompileInput,
		AutonomyProposalDecisionBridgeAuthority,
	},
	mcp::{
		self, TOOL_AUTONOMY_CHALLENGE_PROPOSAL, TOOL_AUTONOMY_COMPILE_PROPOSAL, planning,
		planning::PlanningAuthorityArgs,
	},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp::planning::autonomy) struct AutonomyCompileProposalToolArgs {
	pub(in crate::mcp::planning::autonomy) mode: Option<String>,
	pub(in crate::mcp::planning::autonomy) project_id: Option<String>,
	pub(in crate::mcp::planning::autonomy) proposal: AutonomyProposalCompileArgs,
	#[serde(default)]
	pub(in crate::mcp::planning::autonomy) signal_ids: Vec<String>,
	pub(in crate::mcp::planning::autonomy) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp::planning::autonomy) struct AutonomyProposalCompileArgs {
	pub(in crate::mcp::planning::autonomy) objective_id: String,
	pub(in crate::mcp::planning::autonomy) objective_version: u64,
	pub(in crate::mcp::planning::autonomy) source_family: String,
	pub(in crate::mcp::planning::autonomy) intended_surface: String,
	#[serde(default)]
	pub(in crate::mcp::planning::autonomy) affected_identifiers: Vec<String>,
	pub(in crate::mcp::planning::autonomy) summary: String,
	#[serde(default)]
	pub(in crate::mcp::planning::autonomy) challenge_requirements: Vec<String>,
	#[serde(default)]
	pub(in crate::mcp::planning::autonomy) rejected_alternatives: Vec<String>,
	pub(in crate::mcp::planning::autonomy) rollback_path: String,
	#[serde(default)]
	pub(in crate::mcp::planning::autonomy) weakened_validation_or_review: Vec<String>,
	pub(in crate::mcp::planning::autonomy) created_at: Option<String>,
}
impl AutonomyProposalCompileArgs {
	pub(in crate::mcp::planning::autonomy) fn into_compile_input(
		self,
		project_id: &str,
	) -> Result<AutonomyProposalCompileInput, Value> {
		if self.objective_version == 0 {
			return Err(mcp::invalid_tool_arguments(
				TOOL_AUTONOMY_COMPILE_PROPOSAL,
				"`proposal.objectiveVersion` must be greater than zero.",
			));
		}

		Ok(AutonomyProposalCompileInput {
			project_id: project_id.to_owned(),
			objective_id: self.objective_id,
			objective_version: self.objective_version,
			source_family: self.source_family,
			intended_surface: self.intended_surface,
			affected_identifiers: self.affected_identifiers,
			summary: self.summary,
			challenge_requirements: self.challenge_requirements,
			rejected_alternatives: self.rejected_alternatives,
			rollback_path: self.rollback_path,
			weakened_validation_or_review: self.weakened_validation_or_review,
			created_at: self.created_at.unwrap_or_else(planning::mcp_now_rfc3339),
		})
	}
}

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
