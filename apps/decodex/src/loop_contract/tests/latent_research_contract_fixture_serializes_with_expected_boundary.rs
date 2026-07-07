use crate::loop_contract::{DecisionContractStatus, tests};

#[test]
fn latent_research_contract_fixture_serializes_with_expected_boundary() {
	let contract = tests::latent_research_contract_fixture();

	contract.validate().expect("latent contract should validate");

	assert_eq!(contract.contract_id(), "decision-x-loop-contract");
	assert_eq!(contract.status(), DecisionContractStatus::DraftLatent);
	assert_eq!(contract.source_intent().summary(), "Decide X and shape follow-up work.");
	assert_eq!(contract.accepted_authority().accepted_objectives().len(), 2);
	assert!(contract.execution_readiness().ready_for_issue_shaping());
	assert_eq!(contract.evidence_boundary.private_evidence_refs().len(), 1);
	assert_eq!(contract.evidence_boundary.public_projection_refs().len(), 1);
	assert!(contract.promotion().is_none());
}
