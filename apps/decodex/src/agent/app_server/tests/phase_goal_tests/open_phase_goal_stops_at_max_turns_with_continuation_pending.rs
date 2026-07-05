use crate::agent::app_server::{
	PhaseGoalKind, PhaseGoalRunStatus,
	tests::{self, ContinueTokenCompletionHandler, TestPhaseGoalController},
};

#[test]
fn open_phase_goal_stops_at_max_turns_with_continuation_pending() {
	let handler = ContinueTokenCompletionHandler;
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script = tests::phase_goal_fake_codex_script(&["CONTINUE"], &["active"], &[]);
	let (result, _state_store) = tests::execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 1;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let result = result.expect("open phase goal should exit cleanly at max_turns");

	assert_eq!(result.turn_count, 1);
	assert!(result.continuation_pending);
	assert_eq!(
		result.phase_goal_status,
		Some(PhaseGoalRunStatus {
			phase: PhaseGoalKind::ImplementToValidationReady,
			status: String::from("active"),
		})
	);
}
