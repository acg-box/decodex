use crate::execution_program::{
	ExecutionProgramReadinessContext, ExecutionReadinessState,
	tests::{self},
};

#[test]
fn stale_contract_drift_blocks_direct_dispatch() {
	let stale_node = tests::ready_node("node-stale", "XY-903")
		.with_contract_fingerprint("stale-contract-fingerprint")
		.expect("fingerprint should override");
	let (contract, program) = tests::program_with(vec![stale_node]);
	let evaluation = program
		.evaluate(&contract, &tests::workflow_policy(), &ExecutionProgramReadinessContext::new())
		.expect("program should evaluate");
	let node = &evaluation.nodes()[0];

	assert_eq!(node.state(), ExecutionReadinessState::Stale);
	assert_eq!(node.dispatch_action(), None);
	assert!(evaluation.dispatchable_node_ids().is_empty());
}
