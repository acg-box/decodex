use crate::agent::app_server::{
	PhaseGoalKind,
	tests::{self, TerminalTokenCompletionHandler, TestPhaseGoalController},
};

#[test]
fn phase_goal_completion_accepts_thread_bound_notification_turn_alias() {
	let handler = TerminalTokenCompletionHandler::default();
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script = tests::phase_goal_fake_codex_script_with_notification_turn_mismatch(
		&["DONE", "TERMINAL"],
		&["complete", "complete"],
		&[],
		true,
	);
	let (result, state_store) = tests::execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 3;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let result = result.expect("thread-bound turn alias should still complete phase goals");
	let completed_events = tests::private_phase_goal_events(&state_store, "phase_goal_completed");
	let run_attempt = state_store
		.run_attempt("phase-goal-run")
		.expect("run attempt should load")
		.expect("run attempt should exist");

	assert_eq!(result.turn_count, 2);
	assert_eq!(result.turn_id, "notification-turn-2");
	assert_eq!(run_attempt.turn_id(), Some("notification-turn-2"));
	assert_eq!(
		completed_events.iter().filter_map(|event| event["phase"].as_str()).collect::<Vec<_>>(),
		vec!["implement_to_validation_ready", "handoff_evidence"]
	);
}
