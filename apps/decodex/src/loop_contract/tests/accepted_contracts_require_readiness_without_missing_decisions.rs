use crate::loop_contract::tests::{self};

#[test]
fn accepted_contracts_require_readiness_without_missing_decisions() {
	let mut contract = tests::latent_research_contract_fixture();

	contract.execution_readiness.ready_for_issue_shaping = false;

	let before_failed_promotion = contract.clone();

	assert!(contract.promote(tests::sample_promotion()).is_err());
	assert_eq!(contract, before_failed_promotion, "failed promotion must not mutate the contract");

	let mut contract = tests::latent_research_contract_fixture();

	contract
		.execution_readiness
		.missing_decisions
		.push(String::from("Choose the first generated issue."));

	let before_failed_promotion = contract.clone();

	assert!(contract.promote(tests::sample_promotion()).is_err());
	assert_eq!(contract, before_failed_promotion, "failed promotion must not mutate the contract");

	let mut contract = tests::latent_research_contract_fixture();

	contract.execution_readiness.proposed_issues.clear();

	let before_failed_promotion = contract.clone();

	assert!(contract.promote(tests::sample_promotion()).is_err());
	assert_eq!(contract, before_failed_promotion, "failed promotion must not mutate the contract");
}
