use crate::{
	execution_program::{
		ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionDependencySnapshot,
		ExecutionDispatchAction, ExecutionLinearIssueMapping, ExecutionProgram,
		ExecutionProgramDependency, ExecutionProgramNode, ExecutionProgramNodeStage,
		ExecutionProgramReadinessContext, ExecutionQueueIntent, ExecutionReadinessState,
		ExecutionWorkflowPolicy, intake::ProgramIntakeKind,
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

#[test]
fn readiness_selects_only_startable_ready_nodes() {
	let blocked = ready_node("node-blocked", "XY-901")
		.with_dependencies([
			ExecutionProgramDependency::new("node-ready").expect("dependency should build")
		])
		.expect("dependency should attach");
	let (contract, program) = program_with(vec![ready_node("node-ready", "XY-900"), blocked]);
	let evaluation = program
		.evaluate(&contract, &workflow_policy(), &ExecutionProgramReadinessContext::new())
		.expect("program should evaluate");

	assert_eq!(evaluation.ready_node_ids(), vec!["node-ready"]);
	assert_eq!(evaluation.dispatchable_node_ids(), vec!["node-ready"]);
	assert_eq!(evaluation.nodes()[0].dispatch_action(), Some(ExecutionDispatchAction::Dispatch));
	assert_eq!(evaluation.operator_summary().ready_count, 1);
	assert_eq!(evaluation.operator_summary().blocked_count, 1);
}

#[test]
fn accepted_contract_program_carries_goal_intake_metadata() {
	let (contract, program) = program_with(vec![ready_node("node-ready", "XY-900")]);
	let plan = program.program_intake_plan().expect("new programs should carry intake plan");

	assert_eq!(plan.plan_id(), "program-1");
	assert_eq!(plan.intake_kind(), ProgramIntakeKind::GoalIntake);
	assert_eq!(plan.source_contract_id(), Some(contract.contract_id()));
}

#[test]
fn legacy_execution_program_payload_without_intake_plan_still_validates() {
	let (_contract, program) = program_with(vec![ready_node("node-ready", "XY-900")]);
	let mut payload =
		serde_json::to_value(&program).expect("program payload should serialize to json");

	payload
		.as_object_mut()
		.expect("program payload should be an object")
		.remove("program_intake_plan");

	let legacy_program: ExecutionProgram =
		serde_json::from_value(payload).expect("legacy program should deserialize");

	legacy_program.validate().expect("legacy program should validate");

	assert!(legacy_program.program_intake_plan().is_none());
}

#[test]
fn dependency_blocking_respects_workflow_terminal_states() {
	let dependent = ready_node("node-dependent", "XY-902")
		.with_dependencies([
			ExecutionProgramDependency::new("node-dependency").expect("dependency should build")
		])
		.expect("dependency should attach");
	let (contract, program) =
		program_with(vec![ready_node("node-dependency", "XY-901"), dependent.clone()]);
	let blocked_context = ExecutionProgramReadinessContext::new().with_dependency_snapshots([
		ExecutionDependencySnapshot::tracker_state("node-dependency", "In Review")
			.expect("snapshot should build"),
	]);
	let blocked = program
		.evaluate(&contract, &workflow_policy(), &blocked_context)
		.expect("program should evaluate");
	let dependent_evaluation = blocked
		.nodes()
		.iter()
		.find(|node| node.node_id() == "node-dependent")
		.expect("dependent node should exist");

	assert_eq!(dependent_evaluation.state(), ExecutionReadinessState::Blocked);
	assert!(
		dependent_evaluation
			.reasons()
			.iter()
			.any(|reason| reason.contains("required terminal state"))
	);

	let ready_context = ExecutionProgramReadinessContext::new().with_dependency_snapshots([
		ExecutionDependencySnapshot::tracker_state("node-dependency", "Done")
			.expect("snapshot should build"),
	]);
	let ready = program
		.evaluate(&contract, &workflow_policy(), &ready_context)
		.expect("program should evaluate");

	assert!(ready.dispatchable_node_ids().contains(&"node-dependent"));
}

#[test]
fn stale_contract_drift_blocks_direct_dispatch() {
	let stale_node = ready_node("node-stale", "XY-903")
		.with_contract_fingerprint("stale-contract-fingerprint")
		.expect("fingerprint should override");
	let (contract, program) = program_with(vec![stale_node]);
	let evaluation = program
		.evaluate(&contract, &workflow_policy(), &ExecutionProgramReadinessContext::new())
		.expect("program should evaluate");
	let node = &evaluation.nodes()[0];

	assert_eq!(node.state(), ExecutionReadinessState::Stale);
	assert_eq!(node.dispatch_action(), None);
	assert!(evaluation.dispatchable_node_ids().is_empty());
}

#[test]
fn conflict_domain_blocks_ready_node() {
	let conflict = ExecutionConflictDomain::new(
		ExecutionConflictDomainKind::File,
		"apps/decodex/src/runtime.rs",
	)
	.expect("domain should build");
	let node = ready_node("node-conflict", "XY-904")
		.with_conflict_domains([conflict.clone()])
		.expect("conflict should attach");
	let (contract, program) = program_with(vec![node]);
	let context =
		ExecutionProgramReadinessContext::new().with_occupied_conflict_domains([conflict]);
	let evaluation =
		program.evaluate(&contract, &workflow_policy(), &context).expect("program should evaluate");
	let node = &evaluation.nodes()[0];

	assert_eq!(node.state(), ExecutionReadinessState::Blocked);
	assert!(node.reasons().iter().any(|reason| reason.contains("already occupied")));
}

#[test]
fn unmapped_ready_to_queue_node_is_blocked_from_startable_selection() {
	let unmapped = ExecutionProgramNode::new(
		"node-unmapped",
		ExecutionProgramNodeStage::Runtime,
		"Implement unmapped work.",
		ExecutionQueueIntent::ReadyToQueue,
	)
	.expect("node should build")
	.with_acceptance_expectations([String::from("Acceptance is concrete.")])
	.expect("acceptance should attach")
	.with_validation_expectations([String::from("Run the repo gate.")])
	.expect("validation should attach");
	let (contract, program) = program_with(vec![unmapped]);
	let evaluation = program
		.evaluate(&contract, &workflow_policy(), &ExecutionProgramReadinessContext::new())
		.expect("program should evaluate");
	let node = &evaluation.nodes()[0];

	assert_eq!(node.state(), ExecutionReadinessState::Blocked);
	assert!(node.reasons().iter().any(|reason| reason.contains("no normal Linear issue")));
	assert!(evaluation.dispatchable_node_ids().is_empty());
}

#[test]
fn evaluator_rejects_wrong_service_policy() {
	let (contract, program) = program_with(vec![ready_node("node-ready", "XY-908")]);
	let wrong_service_policy = ExecutionWorkflowPolicy::new(
		"other-service",
		vec![String::from("Todo")],
		vec![String::from("Done")],
		"decodex:manual-only",
		"decodex:needs-attention",
	)
	.expect("workflow policy should build");
	let error = program
		.evaluate(&contract, &wrong_service_policy, &ExecutionProgramReadinessContext::new())
		.expect_err("program should reject mismatched service policy");

	assert!(error.to_string().contains("readiness policy belongs to"));
}
