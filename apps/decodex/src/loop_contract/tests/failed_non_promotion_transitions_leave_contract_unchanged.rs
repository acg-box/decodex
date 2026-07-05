use crate::loop_contract::tests;

#[test]
fn failed_non_promotion_transitions_leave_contract_unchanged() {
	let mut contract = tests::latent_research_contract_fixture();
	let before_failed_human_decision = contract.clone();

	assert!(contract.require_human_decision(" ").is_err());
	assert_eq!(
		contract, before_failed_human_decision,
		"failed human-decision transition must not mutate the contract"
	);

	let before_failed_rejection = contract.clone();

	assert!(contract.reject_or_supersede(Some(String::from(" "))).is_err());
	assert_eq!(
		contract, before_failed_rejection,
		"failed rejection transition must not mutate the contract"
	);
}
