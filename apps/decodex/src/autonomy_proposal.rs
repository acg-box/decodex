//! Versioned dry-run autonomy proposal evidence.

use std::{
	collections::BTreeSet,
	path::{Component, Path},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
	autonomy_objective::{AutonomyObjectiveContract, AutonomyObjectiveState},
	autonomy_signal::{AutonomySignal, AutonomySignalFreshness},
	prelude::{Result, eyre},
};

pub(crate) const AUTONOMY_PROPOSAL_SCHEMA: &str = "decodex.autonomy_proposal/1";

const AUTONOMY_PROPOSAL_RECORD_VERSION: u16 = 1;

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
	SupportAgent,
	InlineSkeptic,
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

		if !challenge.objections.is_empty()
			&& candidate.state == AutonomyProposalState::DecisionCandidate
		{
			candidate.state = AutonomyProposalState::NeedsHumanDecision;
		}

		candidate.challenge_evidence.push(challenge);
		candidate.validate()?;

		*self = candidate;

		Ok(())
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

fn proposal_refusals(
	objective: Option<&AutonomyObjectiveContract>,
	signals: &[AutonomySignal],
	input: &AutonomyProposalCompileInput,
	contradictions: &[String],
) -> Vec<AutonomyProposalRefusal> {
	let mut refusals = Vec::new();

	match objective {
		Some(objective)
			if objective.project_id() == input.project_id
				&& objective.id() == input.objective_id
				&& objective.version() == input.objective_version
				&& objective.state() == AutonomyObjectiveState::Accepted => {
				for signal in signals {
					if signal.project_id() != input.project_id
						|| signal.objective_id() != input.objective_id
						|| signal.objective_version() != input.objective_version
					{
						refusals.push(AutonomyProposalRefusal::new(
							AutonomyProposalRefusalReason::MissingObjective,
							format!(
								"Signal `{}` is not tied to objective `{}` version {}.",
								signal.id(),
								input.objective_id,
								input.objective_version
							),
							vec![signal.id().to_owned()],
						));
					}
					if !objective
						.allowed_signal_kinds()
						.iter()
						.any(|kind| kind == signal.kind().as_str())
					{
						refusals.push(AutonomyProposalRefusal::new(
							AutonomyProposalRefusalReason::DisallowedSignalKind,
							format!(
								"Signal `{}` kind `{}` is outside the accepted objective allowed_signal_kinds.",
								signal.id(),
								signal.kind().as_str()
							),
							vec![signal.id().to_owned()],
						));
					}
					if signal.freshness() != AutonomySignalFreshness::Fresh {
						refusals.push(AutonomyProposalRefusal::new(
							AutonomyProposalRefusalReason::StaleEvidence,
							format!(
								"Signal `{}` freshness is `{}` and requires fresh readback before acceptance.",
								signal.id(),
								signal.freshness().as_str()
							),
							vec![signal.id().to_owned()],
						));
					}
				}

				if !surface_allowed(&input.intended_surface, objective.allowed_surfaces()) {
					refusals.push(AutonomyProposalRefusal::new(
						AutonomyProposalRefusalReason::DisallowedSurface,
						format!(
							"Intended surface `{}` is outside the accepted objective allowed_surfaces.",
							input.intended_surface
						),
						vec![input.objective_id.clone()],
					));
				}
			},
		Some(objective) => refusals.push(AutonomyProposalRefusal::new(
			AutonomyProposalRefusalReason::MissingObjective,
			format!(
				"Objective `{}` version {} exists in state `{}` but is not the accepted exact proposal objective.",
				objective.id(),
				objective.version(),
				objective.state().as_str()
			),
			vec![input.objective_id.clone()],
		)),
		None => refusals.push(AutonomyProposalRefusal::new(
			AutonomyProposalRefusalReason::MissingObjective,
			format!(
				"Objective `{}` version {} is missing.",
				input.objective_id,
				input.objective_version
			),
			vec![input.objective_id.clone()],
		)),
	}

	for contradiction in contradictions {
		refusals.push(AutonomyProposalRefusal::new(
			AutonomyProposalRefusalReason::UnresolvedContradiction,
			format!("Contradiction remains unresolved: {contradiction}"),
			vec![input.objective_id.clone()],
		));
	}
	for note in &input.weakened_validation_or_review {
		refusals.push(AutonomyProposalRefusal::new(
			AutonomyProposalRefusalReason::WeakenedValidationReview,
			format!("Validation or review evidence is weakened: {note}"),
			vec![input.objective_id.clone()],
		));
	}

	refusals
}

fn derive_proposal_state(
	has_signals: bool,
	refusals: &[AutonomyProposalRefusal],
) -> AutonomyProposalState {
	if refusals.iter().any(|refusal| {
		matches!(
			refusal.reason,
			AutonomyProposalRefusalReason::DisallowedSignalKind
				| AutonomyProposalRefusalReason::DisallowedSurface
		)
	}) {
		return AutonomyProposalState::Rejected;
	}
	if refusals
		.iter()
		.any(|refusal| refusal.reason == AutonomyProposalRefusalReason::UnresolvedContradiction)
	{
		return AutonomyProposalState::NeedsHumanDecision;
	}
	if !refusals.is_empty() {
		return AutonomyProposalState::NeedsEvidence;
	}
	if has_signals {
		AutonomyProposalState::DecisionCandidate
	} else {
		AutonomyProposalState::Draft
	}
}

fn surface_allowed(intended_surface: &str, allowed_surfaces: &[String]) -> bool {
	let Some(intended_surface) = normalize_repo_relative_path(intended_surface) else {
		return false;
	};

	allowed_surfaces.iter().any(|surface| {
		normalize_repo_relative_path(surface).is_some_and(|surface| {
			intended_surface == surface
				|| intended_surface
					.strip_prefix(&surface)
					.is_some_and(|suffix| suffix.starts_with('/'))
		})
	})
}

fn normalize_repo_relative_path(value: &str) -> Option<String> {
	let path = Path::new(value);

	if path.is_absolute() {
		return None;
	}

	let mut parts = Vec::new();

	for component in path.components() {
		let Component::Normal(part) = component else {
			return None;
		};

		parts.push(part.to_str()?);
	}

	if parts.is_empty() {
		return None;
	}

	Some(parts.join("/"))
}

fn autonomy_proposal_schema() -> String {
	AUTONOMY_PROPOSAL_SCHEMA.to_owned()
}

const fn autonomy_proposal_record_version() -> u16 {
	AUTONOMY_PROPOSAL_RECORD_VERSION
}

fn autonomy_proposal_id(fingerprint: &str) -> String {
	format!("autonomy_proposal:{fingerprint}")
}

fn autonomy_proposal_fingerprint(proposal: &AutonomyProposal) -> Result<String> {
	let material = serde_json::json!({
		"project_id": proposal.project_id,
		"objective_id": proposal.objective_id,
		"objective_version": proposal.objective_version,
		"source_signal_ids": proposal.source_signal_ids,
		"affected_identifiers": proposal.affected_identifiers,
		"source_family": proposal.source_family,
		"intended_surface": proposal.intended_surface,
	});
	let payload = serde_json::to_vec(&material)?;
	let digest = Sha256::digest(payload);
	let mut hash = String::with_capacity(64);

	for byte in digest {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	Ok(hash)
}

fn validate_compile_input(input: &AutonomyProposalCompileInput) -> Result<()> {
	validate_required("autonomy proposal input.project_id", &input.project_id)?;
	validate_required("autonomy proposal input.objective_id", &input.objective_id)?;
	validate_required("autonomy proposal input.source_family", &input.source_family)?;
	validate_required("autonomy proposal input.intended_surface", &input.intended_surface)?;
	validate_required("autonomy proposal input.summary", &input.summary)?;
	validate_required("autonomy proposal input.rollback_path", &input.rollback_path)?;
	validate_required("autonomy proposal input.created_at", &input.created_at)?;
	validate_string_list(
		"autonomy proposal input.affected_identifiers",
		&input.affected_identifiers,
	)?;
	validate_string_list(
		"autonomy proposal input.challenge_requirements",
		&input.challenge_requirements,
	)?;
	validate_string_list(
		"autonomy proposal input.rejected_alternatives",
		&input.rejected_alternatives,
	)?;
	validate_string_list(
		"autonomy proposal input.weakened_validation_or_review",
		&input.weakened_validation_or_review,
	)?;

	if input.objective_version == 0 {
		eyre::bail!("Autonomy proposal input objective_version must be greater than zero.");
	}

	Ok(())
}

fn validate_required(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(())
}

fn validate_optional_required(name: &str, value: Option<&str>) -> Result<()> {
	if let Some(value) = value {
		validate_required(name, value)?;
	}

	Ok(())
}

fn validate_string_list(name: &str, values: &[String]) -> Result<()> {
	for value in values {
		validate_required(name, value)?;
	}

	Ok(())
}

fn validate_sorted_unique(name: &str, values: &[String]) -> Result<()> {
	validate_string_list(name, values)?;

	let mut seen = BTreeSet::new();
	let mut previous = None;

	for value in values {
		if previous.is_some_and(|previous| previous > value.as_str()) {
			eyre::bail!("{name} must be sorted.");
		}
		if !seen.insert(value.as_str()) {
			eyre::bail!("{name} must not contain duplicates.");
		}

		previous = Some(value.as_str());
	}

	Ok(())
}

fn unique_sorted_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
	values
		.into_iter()
		.map(|value| value.trim().to_owned())
		.filter(|value| !value.is_empty())
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect()
}

#[cfg(test)]
mod tests {
	use std::{collections::BTreeMap, slice};

	use crate::{
		autonomy_objective::{
			AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
		},
		autonomy_proposal::{
			AutonomyProposal, AutonomyProposalChallengeInput, AutonomyProposalChallengeSource,
			AutonomyProposalCompileInput, AutonomyProposalRefusalReason, AutonomyProposalState,
		},
		autonomy_signal::{
			AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
			AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
			AutonomySignalSourceType,
		},
		state::StateStore,
	};

	trait ExpectNone {
		fn expect_none(self, message: &str);
	}

	impl<T> ExpectNone for Option<T> {
		fn expect_none(self, message: &str) {
			assert!(self.is_none(), "{message}");
		}
	}

	fn objective_draft_fixture() -> AutonomyObjectiveContract {
		serde_json::from_value(serde_json::json!({
			"schema": "decodex.autonomy_objective/1",
			"record_version": 1,
			"project_id": "decodex",
			"id": "quality-autonomy",
			"version": 1,
			"state": "draft",
			"summary": "Improve Decodex autonomy quality under explicit authority.",
			"goals": ["Reduce repeated validation and review churn."],
			"non_goals": ["Do not bypass Decision Contract authority."],
			"metrics": ["Validation retry count stays below objective tolerance."],
			"allowed_surfaces": ["apps/decodex/src", "docs/spec"],
			"allowed_signal_kinds": ["runtime_health", "review_feedback_cluster"],
			"validation_gates": ["cargo test -p decodex autonomy_proposal --lib"],
			"review_policy": "independent current-head review required",
			"memory_policy": "read-only source-linked memory only",
			"report_policy": "public-safe summaries only"
		}))
		.expect("draft objective should parse")
	}

	fn objective_fixture() -> AutonomyObjectiveContract {
		let mut objective = objective_draft_fixture();

		objective
			.accept(
				AutonomyObjectiveAcceptance::new(
					"operator",
					AutonomyObjectiveActorKind::User,
					"2026-06-22T00:00:00Z",
					"conversation",
				)
				.expect("acceptance should validate"),
			)
			.expect("objective should accept");

		objective
	}

	fn store_accepted_objective(store: &StateStore) -> AutonomyObjectiveContract {
		store
			.upsert_autonomy_objective_draft("decodex", objective_draft_fixture())
			.expect("objective should store");

		store
			.accept_autonomy_objective_version(
				"decodex",
				"quality-autonomy",
				1,
				AutonomyObjectiveAcceptance::new(
					"operator",
					AutonomyObjectiveActorKind::User,
					"2026-06-22T00:00:00Z",
					"conversation",
				)
				.expect("acceptance should validate"),
			)
			.expect("objective should accept")
			.objective()
			.clone()
	}

	fn signal_input() -> AutonomySignalInput {
		AutonomySignalInput {
			project_id: String::from("decodex"),
			objective_id: String::from("quality-autonomy"),
			objective_version: 1,
			source_type: AutonomySignalSourceType::Runtime,
			source_refs: vec![String::from("status:runtime-health")],
			primary_source_refs: Vec::new(),
			issue_id: Some(String::from("XY-1086")),
			run_id: Some(String::from("xy-1086-attempt-1")),
			attempt_id: Some(String::from("1")),
			head_sha: Some(String::from("3cd19609c44cb18bff9e7a34a2f4853754afcee0")),
			captured_at: String::from("2026-06-22T00:00:00Z"),
			freshness: AutonomySignalFreshness::Fresh,
			summary: String::from("Runtime status readback showed repeated friction."),
			evidence: vec![String::from("status readback retained the repeated friction signal")],
			evidence_class: AutonomySignalEvidenceClass::LiveReadback,
			contradictions: Vec::new(),
			gaps: vec![String::from("No dashboard comparison included.")],
			confidence: AutonomySignalConfidence::Medium,
			privacy: AutonomySignalPrivacy::Team,
			observed_counts: BTreeMap::new(),
			review_evidence: None,
			proposal_only: true,
			created_at: String::from("2026-06-22T00:00:05Z"),
		}
	}

	fn runtime_signal() -> AutonomySignal {
		AutonomySignal::runtime_health(signal_input()).expect("runtime signal should validate")
	}

	fn compile_input() -> AutonomyProposalCompileInput {
		AutonomyProposalCompileInput {
			project_id: String::from("decodex"),
			objective_id: String::from("quality-autonomy"),
			objective_version: 1,
			source_family: String::from("runtime_status"),
			intended_surface: String::from("apps/decodex/src/orchestrator/status.rs"),
			affected_identifiers: vec![
				String::from("OperatorLoopStatus"),
				String::from("operator_status"),
			],
			summary: String::from("Compile a bounded proposal from runtime friction evidence."),
			challenge_requirements: vec![String::from(
				"Support-agent or inline skeptic objections are evidence only.",
			)],
			rejected_alternatives: vec![String::from("Direct Decision Contract promotion.")],
			rollback_path: String::from("Discard the dry-run proposal record."),
			weakened_validation_or_review: Vec::new(),
			created_at: String::from("2026-06-22T00:01:00Z"),
		}
	}

	#[test]
	fn autonomy_proposal_dry_run_candidate_shows_lineage_signals_gates_and_gaps() {
		let objective = objective_fixture();
		let signal = runtime_signal();
		let proposal =
			AutonomyProposal::compile_dry_run(Some(&objective), &[signal], compile_input())
				.expect("proposal should compile");

		assert_eq!(proposal.state(), AutonomyProposalState::DecisionCandidate);
		assert_eq!(proposal.objective_id(), "quality-autonomy");
		assert_eq!(proposal.objective_version(), 1);
		assert_eq!(proposal.allowed_surfaces(), ["apps/decodex/src", "docs/spec"]);
		assert_eq!(proposal.validation_gates(), ["cargo test -p decodex autonomy_proposal --lib"]);
		assert_eq!(proposal.source_signal_ids().len(), 1);
		assert_eq!(proposal.gaps(), ["No dashboard comparison included."]);
		assert!(proposal.contradictions().is_empty());
		assert!(proposal.refusal_reasons().is_empty());

		let dry_run_json = serde_json::to_value(&proposal).expect("proposal should encode");

		assert_eq!(dry_run_json["dry_run"], true);
		assert_eq!(dry_run_json["non_executable"], true);
		assert_eq!(dry_run_json["objective_lineage"]["objective_id"], "quality-autonomy");
		assert_eq!(dry_run_json["source_signals"][0]["signal_id"], proposal.source_signal_ids()[0]);
		assert_eq!(dry_run_json["allowed_surfaces"][0], "apps/decodex/src");
		assert_eq!(dry_run_json["goals"][0], "Reduce repeated validation and review churn.");
		assert_eq!(
			dry_run_json["metrics"][0],
			"Validation retry count stays below objective tolerance."
		);
		assert_eq!(dry_run_json["non_goals"][0], "Do not bypass Decision Contract authority.");
		assert_eq!(
			dry_run_json["review_requirements"][0],
			"independent current-head review required"
		);
		assert_eq!(
			dry_run_json["challenge_requirements"][0],
			"Support-agent or inline skeptic objections are evidence only."
		);
		assert_eq!(dry_run_json["rejected_alternatives"][0], "Direct Decision Contract promotion.");
		assert_eq!(dry_run_json["rollback_path"], "Discard the dry-run proposal record.");
		assert_eq!(
			dry_run_json["validation_gates"][0],
			"cargo test -p decodex autonomy_proposal --lib"
		);
		assert!(dry_run_json["refusal_reasons"].as_array().expect("refusals array").is_empty());
	}

	#[test]
	fn autonomy_proposal_id_ignores_timestamps_signal_order_warning_order_and_challenges() {
		let objective = objective_fixture();
		let signal = runtime_signal();
		let mut second_input = signal_input();

		second_input.source_refs = vec![String::from("status:runtime-health:secondary")];
		second_input.evidence = vec![String::from("secondary readback")];

		let second_signal =
			AutonomySignal::runtime_health(second_input).expect("second signal should validate");
		let mut input_a = compile_input();
		let mut input_b = compile_input();

		input_a.affected_identifiers = vec![String::from("b"), String::from("a")];
		input_a.created_at = String::from("2026-06-22T00:01:00Z");
		input_b.affected_identifiers = vec![String::from("a"), String::from("b")];
		input_b.created_at = String::from("2026-06-22T00:55:00Z");

		let proposal_a = AutonomyProposal::compile_dry_run(
			Some(&objective),
			&[signal.clone(), second_signal.clone()],
			input_a,
		)
		.expect("proposal a should compile");
		let mut proposal_b = AutonomyProposal::compile_dry_run(
			Some(&objective),
			&[second_signal, signal.clone(), signal],
			input_b,
		)
		.expect("proposal b should compile");
		let original_id = proposal_b.id().to_owned();

		proposal_b
			.record_challenge(AutonomyProposalChallengeInput {
				source: AutonomyProposalChallengeSource::InlineSkeptic,
				actor: String::from("inline"),
				summary: String::from("Skeptic noted a possible operator wording gap."),
				objections: Vec::new(),
				evidence_refs: vec![String::from("challenge:inline")],
				recorded_at: String::from("2026-06-22T00:56:00Z"),
			})
			.expect("challenge should record");

		assert_eq!(proposal_a.id(), original_id);
		assert_eq!(proposal_a.fingerprint(), proposal_b.fingerprint());
		assert_eq!(proposal_b.id(), original_id);
	}

	#[test]
	fn autonomy_proposal_refusal_reasons_are_specific_and_inspectable() {
		let objective = objective_fixture();
		let signal = runtime_signal();
		let missing =
			AutonomyProposal::compile_dry_run(None, slice::from_ref(&signal), compile_input())
				.expect("missing objective proposal should compile as refusal");

		assert_eq!(missing.state(), AutonomyProposalState::NeedsEvidence);
		assert!(missing.has_refusal_reason(AutonomyProposalRefusalReason::MissingObjective));

		let mut stale_input = signal_input();

		stale_input.freshness = AutonomySignalFreshness::Stale;

		let stale_signal =
			AutonomySignal::runtime_health(stale_input).expect("stale signal should validate");
		let stale =
			AutonomyProposal::compile_dry_run(Some(&objective), &[stale_signal], compile_input())
				.expect("stale evidence proposal should compile as refusal");

		assert_eq!(stale.state(), AutonomyProposalState::NeedsEvidence);
		assert!(stale.has_refusal_reason(AutonomyProposalRefusalReason::StaleEvidence));

		let mut contradiction_input = signal_input();

		contradiction_input.contradictions =
			vec![String::from("Tracker says closed while runtime says active.")];

		let contradictory_signal = AutonomySignal::runtime_health(contradiction_input)
			.expect("contradictory signal should validate");
		let contradictory = AutonomyProposal::compile_dry_run(
			Some(&objective),
			&[contradictory_signal],
			compile_input(),
		)
		.expect("contradictory proposal should compile as refusal");

		assert_eq!(contradictory.state(), AutonomyProposalState::NeedsHumanDecision);
		assert!(
			contradictory
				.has_refusal_reason(AutonomyProposalRefusalReason::UnresolvedContradiction)
		);

		let mut weakened_input = compile_input();

		weakened_input.weakened_validation_or_review =
			vec![String::from("Review evidence is older than the current head.")];

		let weakened = AutonomyProposal::compile_dry_run(
			Some(&objective),
			slice::from_ref(&signal),
			weakened_input,
		)
		.expect("weakened validation proposal should compile as refusal");

		assert_eq!(weakened.state(), AutonomyProposalState::NeedsEvidence);
		assert!(
			weakened.has_refusal_reason(AutonomyProposalRefusalReason::WeakenedValidationReview)
		);

		let mut disallowed_surface_input = compile_input();

		disallowed_surface_input.intended_surface = String::from("scripts/unowned.rs");

		let disallowed_surface = AutonomyProposal::compile_dry_run(
			Some(&objective),
			slice::from_ref(&signal),
			disallowed_surface_input,
		)
		.expect("disallowed surface proposal should compile as refusal");

		assert_eq!(disallowed_surface.state(), AutonomyProposalState::Rejected);
		assert!(
			disallowed_surface.has_refusal_reason(AutonomyProposalRefusalReason::DisallowedSurface)
		);

		let mut traversal_surface_input = compile_input();

		traversal_surface_input.intended_surface =
			String::from("apps/decodex/src/../../scripts/unowned.rs");

		let traversal_surface = AutonomyProposal::compile_dry_run(
			Some(&objective),
			slice::from_ref(&signal),
			traversal_surface_input,
		)
		.expect("traversal surface proposal should compile as refusal");

		assert_eq!(traversal_surface.state(), AutonomyProposalState::Rejected);
		assert!(
			traversal_surface.has_refusal_reason(AutonomyProposalRefusalReason::DisallowedSurface)
		);

		let docs_signal =
			AutonomySignal::docs_skill_drift(signal_input()).expect("docs signal should validate");
		let disallowed_kind =
			AutonomyProposal::compile_dry_run(Some(&objective), &[docs_signal], compile_input())
				.expect("disallowed signal proposal should compile as refusal");

		assert_eq!(disallowed_kind.state(), AutonomyProposalState::Rejected);
		assert!(
			disallowed_kind.has_refusal_reason(AutonomyProposalRefusalReason::DisallowedSignalKind)
		);
	}

	#[test]
	fn autonomy_proposal_rejects_promoted_state_without_decision_contract_provenance() {
		let objective = objective_fixture();
		let signal = runtime_signal();
		let mut proposal =
			AutonomyProposal::compile_dry_run(Some(&objective), &[signal], compile_input())
				.expect("proposal should compile");

		proposal.state = AutonomyProposalState::AcceptedPromoted;

		assert!(
			proposal
				.validate()
				.expect_err("accepted_promoted should require promotion provenance")
				.to_string()
				.contains("cannot claim accepted_promoted")
		);
	}

	#[test]
	fn autonomy_proposal_challenge_records_objections_without_acceptance_authority() {
		let objective = objective_fixture();
		let signal = runtime_signal();
		let mut proposal =
			AutonomyProposal::compile_dry_run(Some(&objective), &[signal], compile_input())
				.expect("proposal should compile");
		let proposal_id = proposal.id().to_owned();

		proposal
			.record_challenge(AutonomyProposalChallengeInput {
				source: AutonomyProposalChallengeSource::SupportAgent,
				actor: String::from("support-agent"),
				summary: String::from("Support agent challenged the evidence sufficiency."),
				objections: vec![String::from("Needs a fresher operator status readback.")],
				evidence_refs: vec![String::from("challenge:support-agent")],
				recorded_at: String::from("2026-06-22T00:02:00Z"),
			})
			.expect("challenge should record");

		assert_eq!(proposal.id(), proposal_id);
		assert_eq!(proposal.state(), AutonomyProposalState::NeedsHumanDecision);
		assert_eq!(proposal.challenge_evidence().len(), 1);
		assert!(!proposal.challenge_evidence()[0].acceptance_authority);
		assert_eq!(
			proposal.challenge_evidence()[0].objections,
			["Needs a fresher operator status readback."]
		);

		let dry_run_json = serde_json::to_value(&proposal).expect("proposal should encode");

		assert_eq!(dry_run_json["challenge_evidence"][0]["acceptance_authority"], false);
		assert_eq!(
			dry_run_json["challenge_evidence"][0]["objections"][0],
			"Needs a fresher operator status readback."
		);
	}

	#[test]
	fn autonomy_proposal_store_round_trips_without_execution_authority_side_effects() {
		let store = StateStore::open_in_memory().expect("store should open");
		let objective = store_accepted_objective(&store);

		objective.supersession().expect_none("accepted fixture must not have supersession");

		let signal = store
			.record_autonomy_signal("decodex", runtime_signal())
			.expect("signal should store")
			.signal()
			.clone();
		let proposal =
			AutonomyProposal::compile_dry_run(Some(&objective), &[signal], compile_input())
				.expect("proposal should compile");
		let stored = store
			.record_autonomy_proposal("decodex", proposal.clone())
			.expect("proposal should persist");

		assert_eq!(stored.proposal(), &proposal);
		assert_eq!(
			store
				.autonomy_proposal("decodex", proposal.id())
				.expect("proposal read should work")
				.expect("proposal should exist")
				.proposal(),
			&proposal
		);
		assert!(
			store
				.list_decision_contracts_for_project("decodex")
				.expect("decision contracts should list")
				.is_empty()
		);
		assert!(store.list_execution_programs("decodex").expect("programs should list").is_empty());
		assert!(
			store
				.list_program_intake_plans("decodex")
				.expect("intake plans should list")
				.is_empty()
		);
	}

	#[test]
	fn autonomy_proposal_sqlite_round_trips_full_dry_run_record() {
		let tempdir = tempfile::tempdir().expect("tempdir should create");
		let db_path = tempdir.path().join("runtime.sqlite3");
		let stored_proposal = {
			let store = StateStore::open(&db_path).expect("store should open");
			let objective = store_accepted_objective(&store);
			let signal = store
				.record_autonomy_signal("decodex", runtime_signal())
				.expect("signal should store")
				.signal()
				.clone();
			let mut proposal =
				AutonomyProposal::compile_dry_run(Some(&objective), &[signal], compile_input())
					.expect("proposal should compile");

			proposal
				.record_challenge(AutonomyProposalChallengeInput {
					source: AutonomyProposalChallengeSource::SupportAgent,
					actor: String::from("support-agent"),
					summary: String::from("Support agent challenged the evidence sufficiency."),
					objections: vec![String::from("Needs a fresher operator status readback.")],
					evidence_refs: vec![String::from("challenge:support-agent")],
					recorded_at: String::from("2026-06-22T00:02:00Z"),
				})
				.expect("challenge should record");
			store
				.record_autonomy_proposal("decodex", proposal.clone())
				.expect("proposal should persist");

			proposal
		};
		let reopened = StateStore::open(&db_path).expect("store should reopen");
		let readback = reopened
			.autonomy_proposal("decodex", stored_proposal.id())
			.expect("proposal read should work")
			.expect("proposal should exist");

		assert_eq!(readback.proposal(), &stored_proposal);
		assert_eq!(readback.state(), AutonomyProposalState::NeedsHumanDecision);
		assert_eq!(
			reopened
				.recent_autonomy_proposals_for_project("decodex", 1)
				.expect("recent proposals should list")[0]
				.proposal(),
			&stored_proposal
		);
		assert!(
			reopened
				.list_decision_contracts_for_project("decodex")
				.expect("decision contracts should list")
				.is_empty()
		);
		assert!(
			reopened.list_execution_programs("decodex").expect("programs should list").is_empty()
		);
		assert!(
			reopened
				.list_program_intake_plans("decodex")
				.expect("intake plans should list")
				.is_empty()
		);
	}
}
