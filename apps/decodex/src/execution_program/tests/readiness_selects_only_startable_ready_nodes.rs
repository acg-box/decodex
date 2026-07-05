use crate::execution_program::{
	ExecutionDispatchAction, ExecutionProgramDependency, ExecutionProgramReadinessContext,
	tests::{self},
};

#[test]
fn readiness_selects_only_startable_ready_nodes() {
	let blocked = tests::ready_node("node-blocked", "XY-901")
		.with_dependencies([
			ExecutionProgramDependency::new("node-ready").expect("dependency should build")
		])
		.expect("dependency should attach");
	let (contract, program) =
		tests::program_with(vec![tests::ready_node("node-ready", "XY-900"), blocked]);
	let evaluation = program
		.evaluate(&contract, &tests::workflow_policy(), &ExecutionProgramReadinessContext::new())
		.expect("program should evaluate");

	assert_eq!(evaluation.ready_node_ids(), vec!["node-ready"]);
	assert_eq!(evaluation.dispatchable_node_ids(), vec!["node-ready"]);
	assert_eq!(evaluation.nodes()[0].dispatch_action(), Some(ExecutionDispatchAction::Dispatch));
	assert_eq!(evaluation.operator_summary().ready_count, 1);
	assert_eq!(evaluation.operator_summary().blocked_count, 1);
}
