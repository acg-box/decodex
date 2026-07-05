use crate::{
	execution_program::{
		ExecutionLinearIssueMapping, ExecutionProgram, ExecutionProgramNode,
		ExecutionProgramNodeStage, ExecutionQueueIntent,
	},
	loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind},
	state::StateStore,
};

pub(crate) fn latent_decision_contract_fixture() -> DecisionContract {
	serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/decision_contract/research_x_latent_contract.json"
	)))
	.expect("research X latent contract fixture should deserialize")
}

pub(crate) fn sample_decision_promotion() -> DecisionPromotion {
	DecisionPromotion::new(
		"operator",
		DecisionPromotionActorKind::User,
		"2026-06-09T10:00:00Z",
		"conversation",
		Some(String::from("User asked Decodex to push this forward.")),
	)
	.expect("sample promotion should validate")
}
pub(crate) fn sample_execution_program(contract: &DecisionContract) -> ExecutionProgram {
	let node = ExecutionProgramNode::new(
		"runtime-readiness",
		ExecutionProgramNodeStage::Runtime,
		"Implement runtime readiness evaluation.",
		ExecutionQueueIntent::ReadyToQueue,
	)
	.expect("program node should validate")
	.with_acceptance_expectations([String::from("Readiness can explain startability.")])
	.expect("acceptance expectations should attach")
	.with_validation_expectations([String::from("Run the registered repo gate.")])
	.expect("validation expectations should attach")
	.with_linear_issue(
		ExecutionLinearIssueMapping::new("issue-853", "XY-853", "Todo")
			.expect("issue mapping should validate"),
	)
	.expect("issue mapping should attach");

	ExecutionProgram::from_accepted_contract("program-853", "decodex", contract, vec![node])
		.expect("execution program should derive from accepted contract")
}

pub(crate) fn assert_decision_contract_retargeted(reopened: &StateStore) {
	assert_eq!(
		reopened
			.list_decision_contracts_for_issue("pubfi", "linear-id-101")
			.expect("canonical decision contracts should list")
			.len(),
		1
	);
	assert!(
		reopened
			.list_decision_contracts_for_issue("pubfi", "PUB-101")
			.expect("old decision contracts should list")
			.is_empty()
	);
}
