use crate::autonomy_proposal::model::{
	AutonomyProposalAcceptedProjectPolicy, AutonomyProposalAuthorityActorKind,
	AutonomyProposalIssueCandidate,
};

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
