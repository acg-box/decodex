use crate::agent::app_server::{
	PhaseGoalKind, PhaseGoalRunStatus,
	tests::{self, TerminalTokenCompletionHandler, TestPhaseGoalController},
};

#[test]
fn phase_goal_complete_runs_validation_transition_before_handoff_goal() {
	let handler = TerminalTokenCompletionHandler::default();
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script =
		tests::phase_goal_fake_codex_script(&["DONE", "TERMINAL"], &["complete", "complete"], &[]);
	let (result, state_store) = tests::execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 3;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let result = result.expect("completed phase goal should advance to handoff evidence goal");
	let completed_events = tests::private_phase_goal_events(&state_store, "phase_goal_completed");
	let goal_set_events = tests::private_phase_goal_events(&state_store, "phase_goal_set");

	assert_eq!(result.turn_count, 2);
	assert_eq!(result.final_output, "TERMINAL");
	assert_eq!(
		result.phase_goal_status,
		Some(PhaseGoalRunStatus {
			phase: PhaseGoalKind::HandoffEvidence,
			status: String::from("complete"),
		})
	);
	assert_eq!(
		completed_events.iter().filter_map(|event| event["phase"].as_str()).collect::<Vec<_>>(),
		vec!["implement_to_validation_ready", "handoff_evidence"]
	);
	assert_eq!(goal_set_events.len(), 2);
	assert_eq!(goal_set_events[1]["phase"], "handoff_evidence");
}
