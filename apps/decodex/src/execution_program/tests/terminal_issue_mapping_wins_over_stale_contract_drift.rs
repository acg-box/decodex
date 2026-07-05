use crate::execution_program::{
	ExecutionProgramReadinessContext, ExecutionReadinessState,
	tests::{self},
};

#[test]
fn terminal_issue_mapping_wins_over_stale_contract_drift() {
	let terminal_node = tests::ready_node("node-terminal", "XY-903")
		.with_contract_fingerprint("stale-contract-fingerprint")
		.expect("fingerprint should override")
		.with_linear_issue(tests::issue("XY-903", "Done"))
		.expect("terminal issue should attach");
	let (contract, program) = tests::program_with(vec![terminal_node]);
	let evaluation = program
		.evaluate(&contract, &tests::workflow_policy(), &ExecutionProgramReadinessContext::new())
		.expect("program should evaluate");
	let node = &evaluation.nodes()[0];

	assert_eq!(node.state(), ExecutionReadinessState::Completed);
	assert_eq!(node.dispatch_action(), None);
	assert_eq!(evaluation.operator_summary().completed_count, 1);
	assert_eq!(evaluation.operator_summary().stale_count, 0);
}
