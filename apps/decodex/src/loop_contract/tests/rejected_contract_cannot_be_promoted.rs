use crate::loop_contract::{
	DecisionContractStatus, DecisionPromotion,
	tests::{self, sample_promotion},
};

#[test]
fn rejected_contract_cannot_be_promoted() {
	let mut contract = tests::latent_research_contract_fixture();

	contract
		.reject_or_supersede(Some(String::from("research-x-replacement")))
		.expect("contract should reject");

	assert_eq!(contract.status(), DecisionContractStatus::RejectedSuperseded);
	assert_eq!(contract.links().superseded_by_contract_id(), Some("research-x-replacement"));
	assert!(
		contract
			.promote(DecisionPromotion { promotion_reason: None, ..sample_promotion() })
			.is_err()
	);
}
