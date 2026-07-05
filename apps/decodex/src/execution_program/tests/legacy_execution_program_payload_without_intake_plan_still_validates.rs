use crate::execution_program::{
	ExecutionProgram,
	tests::{self},
};

#[test]
fn legacy_execution_program_payload_without_intake_plan_still_validates() {
	let (_contract, program) = tests::program_with(vec![tests::ready_node("node-ready", "XY-900")]);
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
