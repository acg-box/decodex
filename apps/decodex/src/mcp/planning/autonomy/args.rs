use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
	},
	autonomy_proposal::{
		AutonomyProposalAuthorityActorKind, AutonomyProposalChallengeInput,
		AutonomyProposalChallengeSource, AutonomyProposalCompileInput,
		AutonomyProposalDecisionBridgeAuthority,
	},
	autonomy_signal::{
		AutonomySignalConfidence, AutonomySignalEvidenceClass, AutonomySignalFreshness,
		AutonomySignalInput, AutonomySignalKind, AutonomySignalPrivacy,
		AutonomySignalReviewEvidence, AutonomySignalSourceType,
	},
	mcp::{
		TOOL_AUTONOMY_CHALLENGE_PROPOSAL, TOOL_AUTONOMY_COMPILE_PROPOSAL, invalid_tool_arguments,
		non_empty_string, tool_refusal,
	},
};

use super::super::{PlanningAuthorityArgs, mcp_now_rfc3339};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AutonomyDraftObjectiveToolArgs {
	pub(super) mode: Option<String>,
	pub(super) project_id: Option<String>,
	pub(super) objective: AutonomyObjectiveContract,
	pub(super) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AutonomyAcceptObjectiveToolArgs {
	pub(super) mode: Option<String>,
	pub(super) project_id: Option<String>,
	pub(super) objective_id: String,
	pub(super) objective_version: u64,
	pub(super) authority: Option<AutonomyObjectiveAcceptanceArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AutonomyObjectiveAcceptanceArgs {
	pub(super) accepted_by: String,
	pub(super) accepted_by_kind: AutonomyObjectiveActorKind,
	pub(super) accepted_at: Option<String>,
	pub(super) acceptance_source: String,
}

impl AutonomyObjectiveAcceptanceArgs {
	pub(super) fn into_acceptance(self) -> Result<AutonomyObjectiveAcceptance, Value> {
		if self.accepted_by_kind == AutonomyObjectiveActorKind::RuntimePolicy {
			return Err(tool_refusal(
				"objective_acceptance_refused",
				"Runtime-policy Objective Contract acceptance must be resolved from trusted Decodex authority state; caller-supplied runtime_policy acceptance fails closed.",
			));
		}

		AutonomyObjectiveAcceptance::new(
			self.accepted_by,
			self.accepted_by_kind,
			self.accepted_at.unwrap_or_else(mcp_now_rfc3339),
			self.acceptance_source,
		)
		.map_err(|error| {
			tool_refusal(
				"objective_acceptance_refused",
				format!("Objective Contract acceptance authority was refused: {error}"),
			)
		})
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AutonomySubmitSignalToolArgs {
	pub(super) mode: Option<String>,
	pub(super) project_id: Option<String>,
	pub(super) kind: AutonomySignalKind,
	pub(super) signal: AutonomySignalInputArgs,
	pub(super) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AutonomySignalInputArgs {
	pub(super) objective_id: String,
	pub(super) objective_version: u64,
	pub(super) source_type: AutonomySignalSourceType,
	pub(super) source_refs: Vec<String>,
	#[serde(default)]
	pub(super) primary_source_refs: Vec<String>,
	pub(super) issue_id: Option<String>,
	pub(super) run_id: Option<String>,
	pub(super) attempt_id: Option<String>,
	pub(super) head_sha: Option<String>,
	pub(super) captured_at: Option<String>,
	pub(super) freshness: AutonomySignalFreshness,
	pub(super) summary: String,
	pub(super) evidence: Vec<String>,
	pub(super) evidence_class: AutonomySignalEvidenceClass,
	#[serde(default)]
	pub(super) contradictions: Vec<String>,
	#[serde(default)]
	pub(super) gaps: Vec<String>,
	pub(super) confidence: AutonomySignalConfidence,
	pub(super) privacy: AutonomySignalPrivacy,
	#[serde(default)]
	pub(super) observed_counts: BTreeMap<String, u64>,
	pub(super) review_evidence: Option<AutonomySignalReviewEvidence>,
	pub(super) proposal_only: Option<bool>,
	pub(super) created_at: Option<String>,
}

impl AutonomySignalInputArgs {
	pub(super) fn into_signal_input(self, project_id: &str) -> AutonomySignalInput {
		let now = mcp_now_rfc3339();

		AutonomySignalInput {
			project_id: project_id.to_owned(),
			objective_id: self.objective_id,
			objective_version: self.objective_version,
			source_type: self.source_type,
			source_refs: self.source_refs,
			primary_source_refs: self.primary_source_refs,
			issue_id: self.issue_id,
			run_id: self.run_id,
			attempt_id: self.attempt_id,
			head_sha: self.head_sha,
			captured_at: self.captured_at.unwrap_or_else(|| now.clone()),
			freshness: self.freshness,
			summary: self.summary,
			evidence: self.evidence,
			evidence_class: self.evidence_class,
			contradictions: self.contradictions,
			gaps: self.gaps,
			confidence: self.confidence,
			privacy: self.privacy,
			observed_counts: self.observed_counts,
			review_evidence: self.review_evidence,
			proposal_only: self.proposal_only.unwrap_or(true),
			created_at: self.created_at.unwrap_or(now),
		}
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AutonomyCompileProposalToolArgs {
	pub(super) mode: Option<String>,
	pub(super) project_id: Option<String>,
	pub(super) proposal: AutonomyProposalCompileArgs,
	#[serde(default)]
	pub(super) signal_ids: Vec<String>,
	pub(super) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AutonomyProposalCompileArgs {
	pub(super) objective_id: String,
	pub(super) objective_version: u64,
	pub(super) source_family: String,
	pub(super) intended_surface: String,
	#[serde(default)]
	pub(super) affected_identifiers: Vec<String>,
	pub(super) summary: String,
	#[serde(default)]
	pub(super) challenge_requirements: Vec<String>,
	#[serde(default)]
	pub(super) rejected_alternatives: Vec<String>,
	pub(super) rollback_path: String,
	#[serde(default)]
	pub(super) weakened_validation_or_review: Vec<String>,
	pub(super) created_at: Option<String>,
}

impl AutonomyProposalCompileArgs {
	pub(super) fn into_compile_input(
		self,
		project_id: &str,
	) -> Result<AutonomyProposalCompileInput, Value> {
		if self.objective_version == 0 {
			return Err(invalid_tool_arguments(
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
			created_at: self.created_at.unwrap_or_else(mcp_now_rfc3339),
		})
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AutonomyChallengeProposalToolArgs {
	pub(super) mode: Option<String>,
	pub(super) project_id: Option<String>,
	pub(super) proposal_id: String,
	pub(super) challenge: AutonomyProposalChallengeArgs,
	pub(super) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AutonomyProposalChallengeArgs {
	pub(super) source: AutonomyProposalChallengeSource,
	pub(super) actor: String,
	pub(super) summary: String,
	#[serde(default)]
	pub(super) objections: Vec<String>,
	#[serde(default)]
	pub(super) evidence_refs: Vec<String>,
	pub(super) recorded_at: Option<String>,
}

impl AutonomyProposalChallengeArgs {
	pub(super) fn into_challenge_input(self) -> Result<AutonomyProposalChallengeInput, Value> {
		if non_empty_string(Some(self.actor.as_str())).is_none()
			|| non_empty_string(Some(self.summary.as_str())).is_none()
		{
			return Err(invalid_tool_arguments(
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
			recorded_at: self.recorded_at.unwrap_or_else(mcp_now_rfc3339),
		})
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AutonomyRequestPromotionToolArgs {
	pub(super) mode: Option<String>,
	pub(super) project_id: Option<String>,
	pub(super) proposal_id: String,
	pub(super) authority: Option<AutonomyProposalAcceptanceArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AutonomyProposalAcceptanceArgs {
	pub(super) accepted_by: String,
	pub(super) accepted_by_kind: AutonomyProposalAuthorityActorKind,
	pub(super) accepted_at: Option<String>,
	pub(super) acceptance_source: String,
	pub(super) reason: String,
	pub(super) proposal_actor: String,
	pub(super) proposal_actor_kind: AutonomyProposalAuthorityActorKind,
	pub(super) accepted_project_policy: Option<Value>,
}

impl AutonomyProposalAcceptanceArgs {
	pub(super) fn into_decision_bridge_authority(
		self,
	) -> Result<AutonomyProposalDecisionBridgeAuthority, Value> {
		if self.accepted_project_policy.is_some() {
			return Err(tool_refusal(
				"autonomy_policy_authority_refused",
				"acceptedProjectPolicy must be resolved from trusted Decodex authority state; MCP request payloads cannot prove accepted policy authority.",
			));
		}

		AutonomyProposalDecisionBridgeAuthority::new(
			self.accepted_by,
			self.accepted_by_kind,
			self.accepted_at.unwrap_or_else(mcp_now_rfc3339),
			self.acceptance_source,
			self.reason,
			self.proposal_actor,
			self.proposal_actor_kind,
			None,
		)
		.map_err(|error| {
			tool_refusal(
				"autonomy_acceptance_authority_refused",
				format!("Autonomy proposal acceptance authority was refused: {error}"),
			)
		})
	}
}
