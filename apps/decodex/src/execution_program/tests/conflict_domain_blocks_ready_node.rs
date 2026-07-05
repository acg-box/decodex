use crate::execution_program::{
	ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionProgramReadinessContext,
	ExecutionReadinessState,
	tests::{self},
};

#[test]
fn conflict_domain_blocks_ready_node() {
	let conflict = ExecutionConflictDomain::new(
		ExecutionConflictDomainKind::File,
		"apps/decodex/src/runtime.rs",
	)
	.expect("domain should build");
	let node = tests::ready_node("node-conflict", "XY-904")
		.with_conflict_domains([conflict.clone()])
		.expect("conflict should attach");
	let (contract, program) = tests::program_with(vec![node]);
	let context =
		ExecutionProgramReadinessContext::new().with_occupied_conflict_domains([conflict]);
	let evaluation = program
		.evaluate(&contract, &tests::workflow_policy(), &context)
		.expect("program should evaluate");
	let node = &evaluation.nodes()[0];

	assert_eq!(node.state(), ExecutionReadinessState::Blocked);
	assert!(node.reasons().iter().any(|reason| reason.contains("already occupied")));
}
