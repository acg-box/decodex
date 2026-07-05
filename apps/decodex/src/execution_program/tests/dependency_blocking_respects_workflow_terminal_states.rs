use crate::execution_program::{
	ExecutionDependencySnapshot, ExecutionProgramDependency, ExecutionProgramReadinessContext,
	ExecutionReadinessState,
	tests::{self},
};

#[test]
fn dependency_blocking_respects_workflow_terminal_states() {
	let dependent = tests::ready_node("node-dependent", "XY-902")
		.with_dependencies([
			ExecutionProgramDependency::new("node-dependency").expect("dependency should build")
		])
		.expect("dependency should attach");
	let (contract, program) = tests::program_with(vec![
		tests::ready_node("node-dependency", "XY-901"),
		dependent.clone(),
	]);
	let blocked_context = ExecutionProgramReadinessContext::new().with_dependency_snapshots([
		ExecutionDependencySnapshot::tracker_state("node-dependency", "In Review")
			.expect("snapshot should build"),
	]);
	let blocked = program
		.evaluate(&contract, &tests::workflow_policy(), &blocked_context)
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
		.evaluate(&contract, &tests::workflow_policy(), &ready_context)
		.expect("program should evaluate");

	assert!(ready.dispatchable_node_ids().contains(&"node-dependent"));
}
