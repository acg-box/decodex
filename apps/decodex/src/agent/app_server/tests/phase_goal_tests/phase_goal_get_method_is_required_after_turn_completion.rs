use crate::agent::app_server::{
	AppServerPhaseGoalFailure, PhaseGoalKind,
	tests::{self, TestPhaseGoalController},
};

#[test]
fn phase_goal_get_method_is_required_after_turn_completion() {
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script = tests::phase_goal_fake_codex_script(&["DONE"], &[], &["thread/goal/get"]);
	let (result, _state_store) = tests::execute_phase_goal_fake_app_server(script, |request| {
		request.phase_goal_controller = Some(&controller);
	});
	let error = result.expect_err("missing goal get support should fail after the turn");
	let failure = error
		.downcast_ref::<AppServerPhaseGoalFailure>()
		.expect("missing goal support should be a typed phase-goal failure");

	assert_eq!(failure.error_class(), "app_server_phase_goal_unsupported");
	assert!(error.to_string().contains("thread/goal/get"));
}
