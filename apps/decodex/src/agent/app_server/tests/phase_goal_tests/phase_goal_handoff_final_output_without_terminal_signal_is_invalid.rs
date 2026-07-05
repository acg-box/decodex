use crate::agent::app_server::{
	AppServerPhaseGoalFailure, PhaseGoalKind,
	tests::{self, ContinueTokenCompletionHandler, TestPhaseGoalController},
};

#[test]
fn phase_goal_handoff_final_output_without_terminal_signal_is_invalid() {
	let handler = ContinueTokenCompletionHandler;
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script =
		tests::phase_goal_fake_codex_script(&["DONE", "DONE"], &["complete", "complete"], &[]);
	let (result, state_store) = tests::execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 3;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let error = result.expect_err("handoff goal final output cannot replace terminal path");
	let failure = error
		.downcast_ref::<AppServerPhaseGoalFailure>()
		.expect("missing terminal path should be a typed phase-goal failure");
	let goal_set_events = tests::private_phase_goal_events(&state_store, "phase_goal_set");

	assert_eq!(failure.error_class(), "phase_goal_terminal_path_missing");
	assert!(error.to_string().contains("handoff_evidence"));
	assert_eq!(goal_set_events.len(), 2);
	assert_eq!(goal_set_events[1]["phase"], "handoff_evidence");
}
