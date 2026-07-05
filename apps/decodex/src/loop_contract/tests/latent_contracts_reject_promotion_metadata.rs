use crate::loop_contract::tests::{self};

#[test]
fn latent_contracts_reject_promotion_metadata() {
	let mut contract = tests::latent_research_contract_fixture();

	contract.promotion = Some(tests::sample_promotion());

	assert!(contract.validate().is_err());
}
