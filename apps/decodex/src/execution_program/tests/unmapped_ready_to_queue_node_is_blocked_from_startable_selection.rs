use crate::execution_program::{
	ExecutionProgramNode, ExecutionProgramNodeStage, ExecutionProgramReadinessContext,
	ExecutionQueueIntent, ExecutionReadinessState,
	tests::{self},
};

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
	let (contract, program) = tests::program_with(vec![unmapped]);
	let evaluation = program
		.evaluate(&contract, &tests::workflow_policy(), &ExecutionProgramReadinessContext::new())
		.expect("program should evaluate");
	let node = &evaluation.nodes()[0];

	assert_eq!(node.state(), ExecutionReadinessState::Blocked);
	assert!(node.reasons().iter().any(|reason| reason.contains("no normal Linear issue")));
	assert!(evaluation.dispatchable_node_ids().is_empty());
}
