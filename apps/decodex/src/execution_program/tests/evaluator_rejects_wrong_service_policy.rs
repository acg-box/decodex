use crate::execution_program::{
	ExecutionProgramReadinessContext, ExecutionWorkflowPolicy,
	tests::{self},
};

#[test]
fn evaluator_rejects_wrong_service_policy() {
	let (contract, program) = tests::program_with(vec![tests::ready_node("node-ready", "XY-908")]);
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
