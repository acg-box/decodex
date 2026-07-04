use serde::{Deserialize, Serialize};

use crate::autonomy_proposal::{
	model::{
		AutonomyProposalChallengeEvidence, AutonomyProposalIssueCandidate,
		AutonomyProposalObjectiveLineage, AutonomyProposalRefusal, AutonomyProposalSourceSignal,
		AutonomyProposalState,
	},
	validation,
};

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
