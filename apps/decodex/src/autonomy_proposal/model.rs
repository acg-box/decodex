use serde::{Deserialize, Serialize};

use crate::{
	autonomy_proposal::validation,
	prelude::{Result, eyre},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomyProposalAuthorityActorKind {
	User,
	RuntimePolicy,
	ExternalAgent,
}
impl AutonomyProposalAuthorityActorKind {
	pub(in crate::autonomy_proposal) fn as_str(self) -> &'static str {
		match self {
			Self::User => "user",
			Self::RuntimePolicy => "runtime_policy",
			Self::ExternalAgent => "external_agent",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomyProposalAcceptedProjectPolicy {
	pub(crate) project_id: String,
	pub(crate) objective_id: String,
	pub(crate) objective_version: u64,
	pub(crate) accepted_policy_id: String,
	pub(crate) accepted_policy_version: String,
	pub(crate) authority_ref: String,
	pub(crate) authorized_actor: String,
	pub(crate) authorized_actor_kind: AutonomyProposalAuthorityActorKind,
	pub(crate) authorized_acceptance_sources: Vec<String>,
	pub(crate) authorized_scopes: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomyProposalChallengeSource {
	#[serde(alias = "support_agent")]
	Subagent,
	InlineSkeptic,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomyProposalChallengeInput {
	pub(crate) source: AutonomyProposalChallengeSource,
	pub(crate) actor: String,
	pub(crate) summary: String,
	pub(crate) objections: Vec<String>,
	pub(crate) evidence_refs: Vec<String>,
	pub(crate) recorded_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalChallengeEvidence {
	pub(in crate::autonomy_proposal) source: AutonomyProposalChallengeSource,
	pub(in crate::autonomy_proposal) actor: String,
	pub(in crate::autonomy_proposal) summary: String,
	#[serde(default)]
	pub(in crate::autonomy_proposal) objections: Vec<String>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) evidence_refs: Vec<String>,
	pub(in crate::autonomy_proposal) recorded_at: String,
	pub(in crate::autonomy_proposal) acceptance_authority: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomyProposalCompileInput {
	pub(crate) project_id: String,
	pub(crate) objective_id: String,
	pub(crate) objective_version: u64,
	pub(crate) source_family: String,
	pub(crate) intended_surface: String,
	pub(crate) affected_identifiers: Vec<String>,
	pub(crate) summary: String,
	pub(crate) challenge_requirements: Vec<String>,
	pub(crate) rejected_alternatives: Vec<String>,
	pub(crate) rollback_path: String,
	pub(crate) weakened_validation_or_review: Vec<String>,
	pub(crate) issue_candidates: Vec<AutonomyProposalIssueCandidate>,
	pub(crate) created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomyProposalDecisionBridgeAuthorityInput {
	pub(crate) accepted_by: String,
	pub(crate) accepted_by_kind: AutonomyProposalAuthorityActorKind,
	pub(crate) accepted_at: String,
	pub(crate) acceptance_source: String,
	pub(crate) reason: String,
	pub(crate) proposal_actor: String,
	pub(crate) proposal_actor_kind: AutonomyProposalAuthorityActorKind,
	pub(crate) accepted_project_policy: Option<AutonomyProposalAcceptedProjectPolicy>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomyProposalDecisionBridgeAuthority {
	pub(crate) accepted_by: String,
	pub(crate) accepted_by_kind: AutonomyProposalAuthorityActorKind,
	pub(crate) accepted_at: String,
	pub(crate) acceptance_source: String,
	pub(crate) reason: String,
	pub(crate) proposal_actor: String,
	pub(crate) proposal_actor_kind: AutonomyProposalAuthorityActorKind,
	pub(crate) accepted_project_policy: Option<AutonomyProposalAcceptedProjectPolicy>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalIssueCandidate {
	pub(crate) key: String,
	pub(crate) title: String,
	pub(crate) objective: String,
	pub(crate) stage: String,
	#[serde(default)]
	pub(crate) dependencies: Vec<String>,
	#[serde(default)]
	#[serde(alias = "conflictDomains")]
	pub(crate) conflict_domains: Vec<String>,
	pub(crate) acceptance: Vec<String>,
	pub(crate) validation: Vec<String>,
	#[serde(default)]
	pub(crate) risk: Vec<String>,
	#[serde(alias = "queueIntent")]
	pub(crate) queue_intent: String,
}
impl AutonomyProposalIssueCandidate {
	pub(in crate::autonomy_proposal) fn validate(&self) -> Result<()> {
		validation::validate_required("autonomy proposal issue_candidates.key", &self.key)?;
		validation::validate_required("autonomy proposal issue_candidates.title", &self.title)?;
		validation::validate_required(
			"autonomy proposal issue_candidates.objective",
			&self.objective,
		)?;
		validation::validate_required("autonomy proposal issue_candidates.stage", &self.stage)?;
		validation::validate_string_list(
			"autonomy proposal issue_candidates.dependencies",
			&self.dependencies,
		)?;
		validation::validate_string_list(
			"autonomy proposal issue_candidates.conflict_domains",
			&self.conflict_domains,
		)?;
		validation::validate_string_list(
			"autonomy proposal issue_candidates.acceptance",
			&self.acceptance,
		)?;
		validation::validate_string_list(
			"autonomy proposal issue_candidates.validation",
			&self.validation,
		)?;
		validation::validate_string_list("autonomy proposal issue_candidates.risk", &self.risk)?;
		validation::validate_required(
			"autonomy proposal issue_candidates.queue_intent",
			&self.queue_intent,
		)?;

		if self.acceptance.is_empty() {
			eyre::bail!(
				"Autonomy proposal issue candidate `{}` must include acceptance criteria.",
				self.key
			);
		}
		if self.validation.is_empty() {
			eyre::bail!(
				"Autonomy proposal issue candidate `{}` must include validation expectations.",
				self.key
			);
		}

		validation::validate_proposed_issue_stage(&self.key, &self.stage)?;

		validation::validate_proposed_issue_queue_intent(&self.key, &self.queue_intent)
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalObjectiveLineage {
	pub(in crate::autonomy_proposal) project_id: String,
	pub(in crate::autonomy_proposal) objective_id: String,
	pub(in crate::autonomy_proposal) objective_version: u64,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::autonomy_proposal) objective_state: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::autonomy_proposal) objective_summary: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalSourceSignal {
	pub(in crate::autonomy_proposal) signal_id: String,
	pub(in crate::autonomy_proposal) kind: String,
	pub(in crate::autonomy_proposal) freshness: String,
	pub(in crate::autonomy_proposal) evidence_class: String,
	pub(in crate::autonomy_proposal) confidence: String,
	#[serde(default)]
	pub(in crate::autonomy_proposal) gaps: Vec<String>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) contradictions: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomyProposalRefusalReason {
	MissingObjective,
	DisallowedSignalKind,
	DisallowedSurface,
	StaleEvidence,
	UnresolvedContradiction,
	WeakenedValidationReview,
}
impl AutonomyProposalRefusalReason {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::MissingObjective => "missing_objective",
			Self::DisallowedSignalKind => "disallowed_signal_kind",
			Self::DisallowedSurface => "disallowed_surface",
			Self::StaleEvidence => "stale_evidence",
			Self::UnresolvedContradiction => "unresolved_contradiction",
			Self::WeakenedValidationReview => "weakened_validation_review",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalRefusal {
	pub(in crate::autonomy_proposal) reason: AutonomyProposalRefusalReason,
	pub(in crate::autonomy_proposal) detail: String,
	#[serde(default)]
	pub(in crate::autonomy_proposal) evidence_refs: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomyProposalState {
	Draft,
	NeedsEvidence,
	NeedsHumanDecision,
	Rejected,
	DecisionCandidate,
	AcceptedPromoted,
}
impl AutonomyProposalState {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Draft => "draft",
			Self::NeedsEvidence => "needs_evidence",
			Self::NeedsHumanDecision => "needs_human_decision",
			Self::Rejected => "rejected",
			Self::DecisionCandidate => "decision_candidate",
			Self::AcceptedPromoted => "accepted_promoted",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposal {
	#[serde(default = "validation::autonomy_proposal_schema")]
	pub(in crate::autonomy_proposal) schema: String,
	#[serde(default = "validation::autonomy_proposal_record_version")]
	pub(in crate::autonomy_proposal) record_version: u16,
	pub(in crate::autonomy_proposal) id: String,
	pub(in crate::autonomy_proposal) fingerprint: String,
	pub(in crate::autonomy_proposal) project_id: String,
	pub(in crate::autonomy_proposal) objective_id: String,
	pub(in crate::autonomy_proposal) objective_version: u64,
	pub(in crate::autonomy_proposal) state: AutonomyProposalState,
	pub(in crate::autonomy_proposal) source_family: String,
	pub(in crate::autonomy_proposal) intended_surface: String,
	#[serde(default)]
	pub(in crate::autonomy_proposal) affected_identifiers: Vec<String>,
	pub(in crate::autonomy_proposal) summary: String,
	pub(in crate::autonomy_proposal) objective_lineage: AutonomyProposalObjectiveLineage,
	#[serde(default)]
	pub(in crate::autonomy_proposal) source_signal_ids: Vec<String>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) source_signals: Vec<AutonomyProposalSourceSignal>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) allowed_surfaces: Vec<String>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) validation_gates: Vec<String>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) goals: Vec<String>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) metrics: Vec<String>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) non_goals: Vec<String>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) review_requirements: Vec<String>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) challenge_requirements: Vec<String>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) rejected_alternatives: Vec<String>,
	pub(in crate::autonomy_proposal) rollback_path: String,
	#[serde(default)]
	#[serde(skip_serializing_if = "Vec::is_empty")]
	pub(in crate::autonomy_proposal) issue_candidates: Vec<AutonomyProposalIssueCandidate>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) contradictions: Vec<String>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) gaps: Vec<String>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) refusal_reasons: Vec<AutonomyProposalRefusal>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) challenge_evidence: Vec<AutonomyProposalChallengeEvidence>,
	pub(in crate::autonomy_proposal) dry_run: bool,
	pub(in crate::autonomy_proposal) non_executable: bool,
	pub(in crate::autonomy_proposal) created_at: String,
}
