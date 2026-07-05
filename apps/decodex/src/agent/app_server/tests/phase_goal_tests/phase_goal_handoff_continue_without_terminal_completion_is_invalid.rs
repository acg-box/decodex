use crate::agent::app_server::{
	AppServerPhaseGoalFailure, PhaseGoalKind,
	tests::{self, ContinueTokenCompletionHandler, TestPhaseGoalController},
};

#[test]
fn phase_goal_handoff_continue_without_terminal_completion_is_invalid() {
	let handler = ContinueTokenCompletionHandler;
	let controller = TestPhaseGoalController::new(PhaseGoalKind::HandoffEvidence);
	let script = tests::phase_goal_fake_codex_script(&["CONTINUE"], &["complete"], &[]);
	let (result, _state_store) = tests::execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 2;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let error = result.expect_err("handoff goal completion cannot replace terminal path");
	let failure = error
		.downcast_ref::<AppServerPhaseGoalFailure>()
		.expect("missing terminal path should be a typed phase-goal failure");

	assert_eq!(failure.error_class(), "phase_goal_terminal_path_missing");
	assert!(error.to_string().contains("handoff_evidence"));
}
