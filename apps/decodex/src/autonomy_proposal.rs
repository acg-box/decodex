//! Versioned dry-run autonomy proposal evidence.

use std::{
	collections::BTreeSet,
	path::{Component, Path},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
	autonomy_objective::{AutonomyObjectiveContract, AutonomyObjectiveState},
	autonomy_signal::{AutonomySignal, AutonomySignalFreshness},
	loop_contract::{DecisionContract, DecisionContractStatus},
	prelude::{Result, eyre},
};

mod decision;
#[cfg(test)] mod tests;
mod validation;

#[allow(clippy::wildcard_imports)] use decision::*;
#[allow(clippy::wildcard_imports)] use validation::*;

pub(crate) const AUTONOMY_PROPOSAL_SCHEMA: &str = "decodex.autonomy_proposal/1";

const AUTONOMY_PROPOSAL_RECORD_VERSION: u16 = 1;
const AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE: &str = "autonomy_proposal_acceptance";

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomyProposalChallengeSource {
	#[serde(alias = "support_agent")]
	Subagent,
	InlineSkeptic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomyProposalAuthorityActorKind {
	User,
	RuntimePolicy,
	ExternalAgent,
}
impl AutonomyProposalAuthorityActorKind {
	fn as_str(self) -> &'static str {
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
impl AutonomyProposalAcceptedProjectPolicy {
	#[allow(clippy::too_many_arguments)]
	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn new(
		project_id: impl Into<String>,
		objective_id: impl Into<String>,
		objective_version: u64,
		accepted_policy_id: impl Into<String>,
		accepted_policy_version: impl Into<String>,
		authority_ref: impl Into<String>,
		authorized_actor: impl Into<String>,
		authorized_actor_kind: AutonomyProposalAuthorityActorKind,
		authorized_acceptance_sources: Vec<String>,
		authorized_scopes: Vec<String>,
	) -> Result<Self> {
		let policy = Self {
			project_id: project_id.into(),
			objective_id: objective_id.into(),
			objective_version,
			accepted_policy_id: accepted_policy_id.into(),
			accepted_policy_version: accepted_policy_version.into(),
			authority_ref: authority_ref.into(),
			authorized_actor: authorized_actor.into(),
			authorized_actor_kind,
			authorized_acceptance_sources,
			authorized_scopes,
		};

		policy.validate()?;

		Ok(policy)
	}

	fn validate(&self) -> Result<()> {
		validate_required(
			"autonomy proposal accepted project policy.project_id",
			&self.project_id,
		)?;
		validate_required(
			"autonomy proposal accepted project policy.objective_id",
			&self.objective_id,
		)?;
		validate_required(
			"autonomy proposal accepted project policy.accepted_policy_id",
			&self.accepted_policy_id,
		)?;
		validate_required(
			"autonomy proposal accepted project policy.accepted_policy_version",
			&self.accepted_policy_version,
		)?;
		validate_required(
			"autonomy proposal accepted project policy.authority_ref",
			&self.authority_ref,
		)?;
		validate_required(
			"autonomy proposal accepted project policy.authorized_actor",
			&self.authorized_actor,
		)?;
		validate_string_list(
			"autonomy proposal accepted project policy.authorized_acceptance_sources",
			&self.authorized_acceptance_sources,
		)?;
		validate_string_list(
			"autonomy proposal accepted project policy.authorized_scopes",
			&self.authorized_scopes,
		)?;

		if self.objective_version == 0 {
			eyre::bail!(
				"Autonomy proposal accepted project policy objective_version must be greater than zero."
			);
		}
		if !self.authorized_scopes.iter().any(|scope| scope == AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE) {
			eyre::bail!(
				"Autonomy proposal accepted project policy `{}` must authorize `{}` scope.",
				self.authority_ref,
				AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE
			);
		}

		Ok(())
	}

	fn validate_for_authority(
		&self,
		authority: &AutonomyProposalDecisionBridgeAuthority,
	) -> Result<()> {
		self.validate()?;

		if self.authorized_actor != authority.accepted_by
			|| self.authorized_actor_kind != authority.accepted_by_kind
		{
			eyre::bail!(
				"Autonomy proposal accepted project policy `{}` authorizes `{}` ({}) but acceptance is by `{}` ({}).",
				self.authority_ref,
				self.authorized_actor,
				self.authorized_actor_kind.as_str(),
				authority.accepted_by,
				authority.accepted_by_kind.as_str()
			);
		}
		if !self
			.authorized_acceptance_sources
			.iter()
			.any(|source| source == &authority.acceptance_source)
		{
			eyre::bail!(
				"Autonomy proposal accepted project policy `{}` does not authorize acceptance source `{}`.",
				self.authority_ref,
				authority.acceptance_source
			);
		}

		Ok(())
	}

	fn validate_for_proposal(
		&self,
		proposal: &AutonomyProposal,
		authority: &AutonomyProposalDecisionBridgeAuthority,
	) -> Result<()> {
		self.validate_for_authority(authority)?;

		if self.project_id != proposal.project_id
			|| self.objective_id != proposal.objective_id
			|| self.objective_version != proposal.objective_version
		{
			eyre::bail!(
				"Autonomy proposal accepted project policy `{}` does not match proposal `{}` objective lineage.",
				self.authority_ref,
				proposal.id
			);
		}

		Ok(())
	}
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
	pub(crate) created_at: String,
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
impl AutonomyProposalDecisionBridgeAuthority {
	#[allow(clippy::too_many_arguments)]
	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn new(
		accepted_by: impl Into<String>,
		accepted_by_kind: AutonomyProposalAuthorityActorKind,
		accepted_at: impl Into<String>,
		acceptance_source: impl Into<String>,
		reason: impl Into<String>,
		proposal_actor: impl Into<String>,
		proposal_actor_kind: AutonomyProposalAuthorityActorKind,
		accepted_project_policy: Option<AutonomyProposalAcceptedProjectPolicy>,
	) -> Result<Self> {
		let authority = Self {
			accepted_by: accepted_by.into(),
			accepted_by_kind,
			accepted_at: accepted_at.into(),
			acceptance_source: acceptance_source.into(),
			reason: reason.into(),
			proposal_actor: proposal_actor.into(),
			proposal_actor_kind,
			accepted_project_policy,
		};

		authority.validate()?;

		Ok(authority)
	}

	fn validate(&self) -> Result<()> {
		validate_required("autonomy proposal acceptance.accepted_by", &self.accepted_by)?;
		validate_required("autonomy proposal acceptance.accepted_at", &self.accepted_at)?;
		validate_required(
			"autonomy proposal acceptance.acceptance_source",
			&self.acceptance_source,
		)?;
		validate_required("autonomy proposal acceptance.reason", &self.reason)?;
		validate_required("autonomy proposal acceptance.proposal_actor", &self.proposal_actor)?;

		if let Some(policy) = &self.accepted_project_policy {
			policy.validate_for_authority(self)?;
		}

		if matches!(
			self.accepted_by_kind,
			AutonomyProposalAuthorityActorKind::RuntimePolicy
				| AutonomyProposalAuthorityActorKind::ExternalAgent
		) && self.accepted_project_policy.is_none()
		{
			eyre::bail!(
				"Autonomy proposal acceptance by `{}` requires accepted project policy authority.",
				self.accepted_by_kind.as_str()
			);
		}
		if self.proposal_actor_kind == AutonomyProposalAuthorityActorKind::ExternalAgent
			&& self.accepted_by == self.proposal_actor
			&& self.accepted_project_policy.is_none()
		{
			eyre::bail!(
				"External autonomy proposal actor `{}` cannot accept its own proposal without accepted project policy authority.",
				self.accepted_by
			);
		}

		Ok(())
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalObjectiveLineage {
	project_id: String,
	objective_id: String,
	objective_version: u64,
	#[serde(skip_serializing_if = "Option::is_none")]
	objective_state: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	objective_summary: Option<String>,
}
impl AutonomyProposalObjectiveLineage {
	fn validate(&self) -> Result<()> {
		validate_required("autonomy proposal objective lineage.project_id", &self.project_id)?;
		validate_required("autonomy proposal objective lineage.objective_id", &self.objective_id)?;

		if self.objective_version == 0 {
			eyre::bail!("Autonomy proposal objective lineage version must be greater than zero.");
		}

		validate_optional_required(
			"autonomy proposal objective lineage.objective_state",
			self.objective_state.as_deref(),
		)?;

		validate_optional_required(
			"autonomy proposal objective lineage.objective_summary",
			self.objective_summary.as_deref(),
		)
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalSourceSignal {
	signal_id: String,
	kind: String,
	freshness: String,
	evidence_class: String,
	confidence: String,
	#[serde(default)]
	gaps: Vec<String>,
	#[serde(default)]
	contradictions: Vec<String>,
}
impl AutonomyProposalSourceSignal {
	fn from_signal(signal: &AutonomySignal) -> Self {
		Self {
			signal_id: signal.id().to_owned(),
			kind: signal.kind().as_str().to_owned(),
			freshness: signal.freshness().as_str().to_owned(),
			evidence_class: signal.evidence_class().as_str().to_owned(),
			confidence: signal.confidence().as_str().to_owned(),
			gaps: signal.gaps().to_vec(),
			contradictions: signal.contradictions().to_vec(),
		}
	}

	fn validate(&self) -> Result<()> {
		validate_required("autonomy proposal source signal.signal_id", &self.signal_id)?;
		validate_required("autonomy proposal source signal.kind", &self.kind)?;
		validate_required("autonomy proposal source signal.freshness", &self.freshness)?;
		validate_required("autonomy proposal source signal.evidence_class", &self.evidence_class)?;
		validate_required("autonomy proposal source signal.confidence", &self.confidence)?;
		validate_string_list("autonomy proposal source signal.gaps", &self.gaps)?;

		validate_string_list("autonomy proposal source signal.contradictions", &self.contradictions)
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalRefusal {
	reason: AutonomyProposalRefusalReason,
	detail: String,
	#[serde(default)]
	evidence_refs: Vec<String>,
}
impl AutonomyProposalRefusal {
	pub(crate) fn reason(&self) -> AutonomyProposalRefusalReason {
		self.reason
	}

	pub(crate) fn detail(&self) -> &str {
		&self.detail
	}

	pub(crate) fn evidence_refs(&self) -> &[String] {
		&self.evidence_refs
	}

	fn new(
		reason: AutonomyProposalRefusalReason,
		detail: impl Into<String>,
		evidence_refs: Vec<String>,
	) -> Self {
		Self { reason, detail: detail.into(), evidence_refs }
	}

	fn validate(&self) -> Result<()> {
		validate_required("autonomy proposal refusal.detail", &self.detail)?;

		validate_string_list("autonomy proposal refusal.evidence_refs", &self.evidence_refs)
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalChallengeEvidence {
	source: AutonomyProposalChallengeSource,
	actor: String,
	summary: String,
	#[serde(default)]
	objections: Vec<String>,
	#[serde(default)]
	evidence_refs: Vec<String>,
	recorded_at: String,
	acceptance_authority: bool,
}
impl AutonomyProposalChallengeEvidence {
	fn from_input(input: AutonomyProposalChallengeInput) -> Result<Self> {
		let evidence = Self {
			source: input.source,
			actor: input.actor,
			summary: input.summary,
			objections: input.objections,
			evidence_refs: input.evidence_refs,
			recorded_at: input.recorded_at,
			acceptance_authority: false,
		};

		evidence.validate()?;

		Ok(evidence)
	}

	fn validate(&self) -> Result<()> {
		validate_required("autonomy proposal challenge.actor", &self.actor)?;
		validate_required("autonomy proposal challenge.summary", &self.summary)?;
		validate_required("autonomy proposal challenge.recorded_at", &self.recorded_at)?;
		validate_string_list("autonomy proposal challenge.objections", &self.objections)?;
		validate_string_list("autonomy proposal challenge.evidence_refs", &self.evidence_refs)?;

		if self.acceptance_authority {
			eyre::bail!("Autonomy proposal challenge evidence cannot be acceptance authority.");
		}

		Ok(())
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposal {
	#[serde(default = "autonomy_proposal_schema")]
	schema: String,
	#[serde(default = "autonomy_proposal_record_version")]
	record_version: u16,
	id: String,
	fingerprint: String,
	project_id: String,
	objective_id: String,
	objective_version: u64,
	state: AutonomyProposalState,
	source_family: String,
	intended_surface: String,
	#[serde(default)]
	affected_identifiers: Vec<String>,
	summary: String,
	objective_lineage: AutonomyProposalObjectiveLineage,
	#[serde(default)]
	source_signal_ids: Vec<String>,
	#[serde(default)]
	source_signals: Vec<AutonomyProposalSourceSignal>,
	#[serde(default)]
	allowed_surfaces: Vec<String>,
	#[serde(default)]
	validation_gates: Vec<String>,
	#[serde(default)]
	goals: Vec<String>,
	#[serde(default)]
	metrics: Vec<String>,
	#[serde(default)]
	non_goals: Vec<String>,
	#[serde(default)]
	review_requirements: Vec<String>,
	#[serde(default)]
	challenge_requirements: Vec<String>,
	#[serde(default)]
	rejected_alternatives: Vec<String>,
	rollback_path: String,
	#[serde(default)]
	contradictions: Vec<String>,
	#[serde(default)]
	gaps: Vec<String>,
	#[serde(default)]
	refusal_reasons: Vec<AutonomyProposalRefusal>,
	#[serde(default)]
	challenge_evidence: Vec<AutonomyProposalChallengeEvidence>,
	dry_run: bool,
	non_executable: bool,
	created_at: String,
}
#[allow(dead_code)]
impl AutonomyProposal {
	pub(crate) fn compile_dry_run(
		objective: Option<&AutonomyObjectiveContract>,
		signals: &[AutonomySignal],
		input: AutonomyProposalCompileInput,
	) -> Result<Self> {
		validate_compile_input(&input)?;

		for signal in signals {
			signal.validate()?;
		}

		let objective_lineage = AutonomyProposalObjectiveLineage {
			project_id: input.project_id.clone(),
			objective_id: input.objective_id.clone(),
			objective_version: input.objective_version,
			objective_state: objective.map(|objective| objective.state().as_str().to_owned()),
			objective_summary: objective.map(|objective| objective.summary().to_owned()),
		};
		let mut source_signals =
			signals.iter().map(AutonomyProposalSourceSignal::from_signal).collect::<Vec<_>>();

		source_signals.sort_by(|left, right| left.signal_id.cmp(&right.signal_id));
		source_signals.dedup_by(|left, right| left.signal_id == right.signal_id);

		let source_signal_ids =
			unique_sorted_strings(source_signals.iter().map(|signal| signal.signal_id.clone()));
		let allowed_surfaces =
			objective.map(|objective| objective.allowed_surfaces().to_vec()).unwrap_or_default();
		let validation_gates =
			objective.map(|objective| objective.validation_gates().to_vec()).unwrap_or_default();
		let goals = objective.map(|objective| objective.goals().to_vec()).unwrap_or_default();
		let metrics = objective.map(|objective| objective.metrics().to_vec()).unwrap_or_default();
		let non_goals =
			objective.map(|objective| objective.non_goals().to_vec()).unwrap_or_default();
		let review_requirements = objective
			.map(|objective| vec![objective.review_policy().to_owned()])
			.unwrap_or_default();
		let contradictions = unique_sorted_strings(
			signals.iter().flat_map(|signal| signal.contradictions().to_vec()),
		);
		let gaps = unique_sorted_strings(signals.iter().flat_map(|signal| signal.gaps().to_vec()));
		let refusal_reasons = proposal_refusals(objective, signals, &input, &contradictions);
		let state = derive_proposal_state(!source_signal_ids.is_empty(), &refusal_reasons);
		let affected_identifiers = unique_sorted_strings(input.affected_identifiers);
		let mut proposal = Self {
			schema: autonomy_proposal_schema(),
			record_version: autonomy_proposal_record_version(),
			id: String::new(),
			fingerprint: String::new(),
			project_id: input.project_id,
			objective_id: input.objective_id,
			objective_version: input.objective_version,
			state,
			source_family: input.source_family,
			intended_surface: input.intended_surface,
			affected_identifiers,
			summary: input.summary,
			objective_lineage,
			source_signal_ids,
			source_signals,
			allowed_surfaces,
			validation_gates,
			goals,
			metrics,
			non_goals,
			review_requirements,
			challenge_requirements: unique_sorted_strings(input.challenge_requirements),
			rejected_alternatives: unique_sorted_strings(input.rejected_alternatives),
			rollback_path: input.rollback_path,
			contradictions,
			gaps,
			refusal_reasons,
			challenge_evidence: Vec::new(),
			dry_run: true,
			non_executable: true,
			created_at: input.created_at,
		};
		let fingerprint = autonomy_proposal_fingerprint(&proposal)?;

		proposal.id = autonomy_proposal_id(&fingerprint);
		proposal.fingerprint = fingerprint;

		proposal.validate()?;

		Ok(proposal)
	}

	pub(crate) fn id(&self) -> &str {
		&self.id
	}

	pub(crate) fn fingerprint(&self) -> &str {
		&self.fingerprint
	}

	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn objective_id(&self) -> &str {
		&self.objective_id
	}

	pub(crate) fn objective_version(&self) -> u64 {
		self.objective_version
	}

	pub(crate) fn state(&self) -> AutonomyProposalState {
		self.state
	}

	pub(crate) fn source_family(&self) -> &str {
		&self.source_family
	}

	pub(crate) fn intended_surface(&self) -> &str {
		&self.intended_surface
	}

	pub(crate) fn affected_identifiers(&self) -> &[String] {
		&self.affected_identifiers
	}

	pub(crate) fn summary(&self) -> &str {
		&self.summary
	}

	pub(crate) fn source_signal_ids(&self) -> &[String] {
		&self.source_signal_ids
	}

	pub(crate) fn allowed_surfaces(&self) -> &[String] {
		&self.allowed_surfaces
	}

	pub(crate) fn validation_gates(&self) -> &[String] {
		&self.validation_gates
	}

	pub(crate) fn contradictions(&self) -> &[String] {
		&self.contradictions
	}

	pub(crate) fn gaps(&self) -> &[String] {
		&self.gaps
	}

	pub(crate) fn refusal_reasons(&self) -> &[AutonomyProposalRefusal] {
		&self.refusal_reasons
	}

	pub(crate) fn challenge_evidence(&self) -> &[AutonomyProposalChallengeEvidence] {
		&self.challenge_evidence
	}

	pub(crate) fn has_refusal_reason(&self, reason: AutonomyProposalRefusalReason) -> bool {
		self.refusal_reasons.iter().any(|refusal| refusal.reason == reason)
	}

	pub(crate) fn record_challenge(&mut self, input: AutonomyProposalChallengeInput) -> Result<()> {
		let challenge = AutonomyProposalChallengeEvidence::from_input(input)?;
		let mut candidate = self.clone();

		candidate.challenge_evidence.push(challenge);
		candidate.validate()?;

		*self = candidate;

		Ok(())
	}

	pub(crate) fn to_decision_contract_candidate(
		&self,
		authority: AutonomyProposalDecisionBridgeAuthority,
	) -> Result<DecisionContract> {
		self.validate()?;
		authority.validate()?;

		if let Some(policy) = &authority.accepted_project_policy {
			policy.validate_for_proposal(self, &authority)?;
		}

		if self.state != AutonomyProposalState::DecisionCandidate {
			eyre::bail!(
				"Autonomy proposal `{}` is `{}` and cannot become a Decision Contract candidate.",
				self.id,
				self.state.as_str()
			);
		}
		if !self.refusal_reasons.is_empty() {
			eyre::bail!(
				"Autonomy proposal `{}` has refusal reasons and cannot become a Decision Contract candidate.",
				self.id
			);
		}

		let payload = serde_json::json!({
			"schema": crate::loop_contract::DECISION_CONTRACT_SCHEMA,
			"record_version": crate::loop_contract::DECISION_CONTRACT_RECORD_VERSION,
			"contract_id": self.decision_contract_id(),
			"status": DecisionContractStatus::DraftLatent.as_str(),
			"source_intent": {
				"summary": format!("Accepted autonomy proposal: {}", self.summary),
				"user_utterance": authority.reason.clone(),
				"source_issue_identifier": proposal_source_issue_identifier(&self.affected_identifiers),
			},
			"research_provenance": autonomy_decision_research_provenance(self, &authority),
			"research_evidence": autonomy_decision_research_evidence(self),
			"research_options": autonomy_decision_research_options(self),
			"accepted_authority": {
				"accepted_objectives": proposal_objectives(self),
				"non_goals": self.non_goals.clone(),
				"constraints": proposal_constraints(self),
				"assumptions": proposal_assumptions(self, &authority),
				"objections": proposal_objections(self),
				"stop_conditions": proposal_stop_conditions(self),
			},
			"execution_readiness": {
				"summary": "Accepted autonomy proposal is ready for normal Decision Contract promotion.",
				"ready_for_issue_shaping": true,
				"missing_decisions": [],
				"validation_expectations": proposal_validation_expectations(self),
				"risk_notes": proposal_risk_notes(self),
				"proposed_issues": [proposal_issue_candidate(self)],
				"promotion_targets": ["research_promote", "decision_contract"],
				"conflict_domains": proposal_conflict_domains(self),
			},
			"links": {
				"generated_issue_ids": [],
				"generated_issue_identifiers": [],
				"execution_program_node_ids": [],
			},
			"evidence_boundary": {
				"private_evidence_refs": [],
				"public_projection_refs": [
					{
						"surface": "autonomy_proposal",
						"reference": self.id.clone(),
						"summary": "Accepted autonomy proposal converted to latent Decision Contract candidate."
					}
				],
				"public_summary": "Autonomy proposal preserved as a latent Decision Contract candidate."
			},
		});
		let contract = serde_json::from_value::<DecisionContract>(payload)?;

		contract.validate()?;

		Ok(contract)
	}

	pub(crate) fn validate(&self) -> Result<()> {
		validate_required("autonomy proposal schema", &self.schema)?;
		validate_required("autonomy proposal id", &self.id)?;
		validate_required("autonomy proposal fingerprint", &self.fingerprint)?;
		validate_required("autonomy proposal project_id", &self.project_id)?;
		validate_required("autonomy proposal objective_id", &self.objective_id)?;
		validate_required("autonomy proposal source_family", &self.source_family)?;
		validate_required("autonomy proposal intended_surface", &self.intended_surface)?;
		validate_required("autonomy proposal summary", &self.summary)?;
		validate_required("autonomy proposal rollback_path", &self.rollback_path)?;
		validate_required("autonomy proposal created_at", &self.created_at)?;

		if self.schema != AUTONOMY_PROPOSAL_SCHEMA {
			eyre::bail!(
				"Autonomy proposal `{}` has unsupported schema `{}`.",
				self.id,
				self.schema
			);
		}
		if self.record_version != AUTONOMY_PROPOSAL_RECORD_VERSION {
			eyre::bail!(
				"Autonomy proposal `{}` has unsupported record_version `{}`.",
				self.id,
				self.record_version
			);
		}
		if self.objective_version == 0 {
			eyre::bail!(
				"Autonomy proposal `{}` objective_version must be greater than zero.",
				self.id
			);
		}
		if !self.dry_run || !self.non_executable {
			eyre::bail!(
				"Autonomy proposal `{}` must remain non-executable dry-run evidence.",
				self.id
			);
		}
		if self.state == AutonomyProposalState::AcceptedPromoted {
			eyre::bail!(
				"Autonomy proposal `{}` cannot claim accepted_promoted in schema version {} without explicit Decision Contract promotion provenance.",
				self.id,
				self.record_version
			);
		}
		if self.objective_lineage.project_id != self.project_id
			|| self.objective_lineage.objective_id != self.objective_id
			|| self.objective_lineage.objective_version != self.objective_version
		{
			eyre::bail!(
				"Autonomy proposal `{}` objective lineage must match proposal key.",
				self.id
			);
		}

		self.objective_lineage.validate()?;

		validate_sorted_unique("autonomy proposal source_signal_ids", &self.source_signal_ids)?;
		validate_sorted_unique(
			"autonomy proposal affected_identifiers",
			&self.affected_identifiers,
		)?;
		validate_string_list("autonomy proposal allowed_surfaces", &self.allowed_surfaces)?;
		validate_string_list("autonomy proposal validation_gates", &self.validation_gates)?;
		validate_string_list("autonomy proposal goals", &self.goals)?;
		validate_string_list("autonomy proposal metrics", &self.metrics)?;
		validate_string_list("autonomy proposal non_goals", &self.non_goals)?;
		validate_string_list("autonomy proposal review_requirements", &self.review_requirements)?;
		validate_sorted_unique(
			"autonomy proposal challenge_requirements",
			&self.challenge_requirements,
		)?;
		validate_sorted_unique(
			"autonomy proposal rejected_alternatives",
			&self.rejected_alternatives,
		)?;
		validate_sorted_unique("autonomy proposal contradictions", &self.contradictions)?;
		validate_sorted_unique("autonomy proposal gaps", &self.gaps)?;

		let signal_ids_from_refs =
			self.source_signals.iter().map(|signal| signal.signal_id.clone()).collect::<Vec<_>>();

		if signal_ids_from_refs != self.source_signal_ids {
			eyre::bail!(
				"Autonomy proposal `{}` source_signal_ids must match source_signals.",
				self.id
			);
		}

		for signal in &self.source_signals {
			signal.validate()?;
		}
		for refusal in &self.refusal_reasons {
			refusal.validate()?;
		}
		for challenge in &self.challenge_evidence {
			challenge.validate()?;
		}

		let expected = autonomy_proposal_fingerprint(self)?;

		if expected != self.fingerprint {
			eyre::bail!(
				"Autonomy proposal `{}` fingerprint mismatch: expected `{expected}`.",
				self.id
			);
		}

		let expected_id = autonomy_proposal_id(&expected);

		if expected_id != self.id {
			eyre::bail!(
				"Autonomy proposal id `{}` does not match fingerprint `{expected}`.",
				self.id
			);
		}

		Ok(())
	}
}

impl AutonomyProposal {
	fn decision_contract_id(&self) -> String {
		format!("autonomy-decision-{}", &self.fingerprint[..32])
	}
}
