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

mod authority;
mod decision;
mod evidence;
mod proposal;
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalRefusal {
	reason: AutonomyProposalRefusalReason,
	detail: String,
	#[serde(default)]
	evidence_refs: Vec<String>,
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
