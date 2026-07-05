use crate::execution_program::{
	intake::ProgramIntakeKind,
	tests::{self},
};

#[test]
fn accepted_contract_program_carries_goal_intake_metadata() {
	let (contract, program) = tests::program_with(vec![tests::ready_node("node-ready", "XY-900")]);
	let plan = program.program_intake_plan().expect("new programs should carry intake plan");

	assert_eq!(plan.plan_id(), "program-1");
	assert_eq!(plan.intake_kind(), ProgramIntakeKind::GoalIntake);
	assert_eq!(plan.source_contract_id(), Some(contract.contract_id()));
}
