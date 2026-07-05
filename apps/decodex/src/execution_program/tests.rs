mod accepted_contract_program_carries_goal_intake_metadata;
mod conflict_domain_blocks_ready_node;
mod dependency_blocking_respects_workflow_terminal_states;
mod evaluator_rejects_wrong_service_policy;
mod legacy_execution_program_payload_without_intake_plan_still_validates;
mod readiness_selects_only_startable_ready_nodes;
mod stale_contract_drift_blocks_direct_dispatch;
mod terminal_issue_mapping_wins_over_stale_contract_drift;
mod unmapped_ready_to_queue_node_is_blocked_from_startable_selection;

use crate::{
	execution_program::{
		ExecutionLinearIssueMapping, ExecutionProgram, ExecutionProgramNode,
		ExecutionProgramNodeStage, ExecutionQueueIntent, ExecutionWorkflowPolicy,
	},
	loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind},
};

fn latent_contract_fixture() -> DecisionContract {
	serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/decision_contract/research_x_latent_contract.json"
	)))
	.expect("decision contract fixture should deserialize")
}

fn accepted_contract_fixture() -> DecisionContract {
	let mut contract = latent_contract_fixture();

	contract
		.promote(
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-09T10:00:00Z",
				"conversation",
				Some(String::from("User asked to push this forward.")),
			)
			.expect("promotion should build"),
		)
		.expect("contract should promote");

	contract
}

fn workflow_policy() -> ExecutionWorkflowPolicy {
	ExecutionWorkflowPolicy::new(
		"decodex",
		vec![String::from("Todo")],
		vec![String::from("Done"), String::from("Canceled"), String::from("Duplicate")],
		"decodex:manual-only",
		"decodex:needs-attention",
	)
	.expect("workflow policy should build")
}

fn issue(identifier: &str, state: &str) -> ExecutionLinearIssueMapping {
	ExecutionLinearIssueMapping::new(
		format!("linear-{identifier}"),
		identifier.to_owned(),
		state.to_owned(),
	)
	.expect("issue mapping should build")
}

fn ready_node(id: &str, issue_identifier: &str) -> ExecutionProgramNode {
	ExecutionProgramNode::new(
		id,
		ExecutionProgramNodeStage::Runtime,
		format!("Implement {id}."),
		ExecutionQueueIntent::ReadyToQueue,
	)
	.expect("node should build")
	.with_objective_lineage([String::from("Ship the accepted runtime work.")])
	.expect("lineage should attach")
	.with_acceptance_expectations([String::from("Acceptance is concrete.")])
	.expect("acceptance should attach")
	.with_validation_expectations([String::from("Run the repo gate.")])
	.expect("validation should attach")
	.with_linear_issue(issue(issue_identifier, "Todo"))
	.expect("issue should attach")
}

fn program_with(nodes: Vec<ExecutionProgramNode>) -> (DecisionContract, ExecutionProgram) {
	let contract = accepted_contract_fixture();
	let program =
		ExecutionProgram::from_accepted_contract("program-1", "decodex", &contract, nodes)
			.expect("program should derive from accepted contract");

	(contract, program)
}
