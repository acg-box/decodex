mod accepted_authority;
mod evidence;
mod options;
mod provenance;
mod readiness;

use serde_json::Value;

use crate::autonomy_proposal::{AutonomyProposal, AutonomyProposalDecisionBridgeAuthority};

pub(super) fn autonomy_decision_research_provenance(
	proposal: &AutonomyProposal,
	authority: &AutonomyProposalDecisionBridgeAuthority,
) -> Vec<Value> {
	provenance::autonomy_decision_research_provenance(proposal, authority)
}

pub(super) fn autonomy_decision_research_evidence(proposal: &AutonomyProposal) -> Vec<Value> {
	evidence::autonomy_decision_research_evidence(proposal)
}

pub(super) fn autonomy_decision_research_options(proposal: &AutonomyProposal) -> Vec<Value> {
	options::autonomy_decision_research_options(proposal)
}

pub(super) fn proposal_objectives(proposal: &AutonomyProposal) -> Vec<String> {
	accepted_authority::proposal_objectives(proposal)
}

pub(super) fn proposal_constraints(proposal: &AutonomyProposal) -> Vec<String> {
	accepted_authority::proposal_constraints(proposal)
}

pub(super) fn proposal_assumptions(
	proposal: &AutonomyProposal,
	authority: &AutonomyProposalDecisionBridgeAuthority,
) -> Vec<String> {
	accepted_authority::proposal_assumptions(proposal, authority)
}

pub(super) fn proposal_objections(proposal: &AutonomyProposal) -> Vec<String> {
	accepted_authority::proposal_objections(proposal)
}

pub(super) fn proposal_stop_conditions(proposal: &AutonomyProposal) -> Vec<String> {
	accepted_authority::proposal_stop_conditions(proposal)
}

pub(super) fn proposal_validation_expectations(proposal: &AutonomyProposal) -> Vec<String> {
	readiness::proposal_validation_expectations(proposal)
}

pub(super) fn proposal_risk_notes(proposal: &AutonomyProposal) -> Vec<String> {
	readiness::proposal_risk_notes(proposal)
}

pub(super) fn proposal_issue_candidates(proposal: &AutonomyProposal) -> Vec<Value> {
	readiness::proposal_issue_candidates(proposal)
}

pub(super) fn proposal_conflict_domains(proposal: &AutonomyProposal) -> Vec<String> {
	readiness::proposal_conflict_domains(proposal)
}

pub(super) fn proposal_source_issue_identifier(affected_identifiers: &[String]) -> Option<String> {
	readiness::proposal_source_issue_identifier(affected_identifiers)
}
