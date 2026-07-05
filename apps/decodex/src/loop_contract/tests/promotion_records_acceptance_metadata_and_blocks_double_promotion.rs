use crate::loop_contract::{
	DecisionContractStatus,
	tests::{self},
};

#[test]
fn promotion_records_acceptance_metadata_and_blocks_double_promotion() {
	let mut contract = tests::latent_research_contract_fixture();

	contract.promote(tests::sample_promotion()).expect("latent contract should promote");

	assert_eq!(contract.status(), DecisionContractStatus::AcceptedPromoted);
	assert_eq!(
		contract.promotion().expect("promotion should exist").accepted_at(),
		"2026-06-09T10:00:00Z"
	);
	assert!(
		contract.promote(contract.promotion().expect("promotion should exist").clone()).is_err()
	);
}
