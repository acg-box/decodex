use crate::agent::app_server::{
	AppServerPhaseGoalFailure, PhaseGoalKind, PhaseGoalRunStatus,
	tests::{
		self, ContinueTokenCompletionHandler, TerminalTokenCompletionHandler,
		TestPhaseGoalController,
	},
};

#[test]
fn phase_goal_set_method_is_required_when_phase_controller_is_present() {
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script = tests::phase_goal_fake_codex_script(&["DONE"], &[], &["thread/goal/set"]);
	let (result, _state_store) = tests::execute_phase_goal_fake_app_server(script, |request| {
		request.phase_goal_controller = Some(&controller);
	});
	let error = result.expect_err("missing goal set support should fail immediately");
	let failure = error
		.downcast_ref::<AppServerPhaseGoalFailure>()
		.expect("missing goal support should be a typed phase-goal failure");

	assert_eq!(failure.error_class(), "app_server_phase_goal_unsupported");
	assert!(error.to_string().contains("thread/goal/set"));
}

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

#[test]
fn phase_goal_review_repair_validation_transitions_before_repair_evidence_goal() {
	let handler = TerminalTokenCompletionHandler::default();
	let controller = TestPhaseGoalController::new(PhaseGoalKind::RepairAcceptedReviewFindings);
	let script =
		tests::phase_goal_fake_codex_script(&["DONE", "TERMINAL"], &["complete", "complete"], &[]);
	let (result, state_store) = tests::execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 3;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let result =
		result.expect("completed review repair goal should advance to repair evidence goal");
	let completed_events = tests::private_phase_goal_events(&state_store, "phase_goal_completed");
	let goal_set_events = tests::private_phase_goal_events(&state_store, "phase_goal_set");

	assert_eq!(result.turn_count, 2);
	assert_eq!(result.final_output, "TERMINAL");
	assert_eq!(
		result.phase_goal_status,
		Some(PhaseGoalRunStatus {
			phase: PhaseGoalKind::ReviewRepairEvidence,
			status: String::from("complete"),
		})
	);
	assert_eq!(
		completed_events.iter().filter_map(|event| event["phase"].as_str()).collect::<Vec<_>>(),
		vec!["repair_accepted_review_findings", "review_repair_evidence"]
	);
	assert_eq!(goal_set_events.len(), 2);
	assert_eq!(goal_set_events[1]["phase"], "review_repair_evidence");
}

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

#[test]
fn open_phase_goal_stops_at_max_turns_without_terminal_signal() {
	let handler = ContinueTokenCompletionHandler;
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script =
		tests::phase_goal_fake_codex_script(&["CONTINUE", "DONE"], &["active", "active"], &[]);
	let (result, _state_store) = tests::execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 2;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let result = result.expect("open phase goal should allow another bounded turn");

	assert_eq!(result.turn_count, 2);
	assert_eq!(result.turn_id, "turn-2");
	assert_eq!(result.final_output, "DONE");
	assert!(result.continuation_pending);
	assert_eq!(
		result.phase_goal_status,
		Some(PhaseGoalRunStatus {
			phase: PhaseGoalKind::ImplementToValidationReady,
			status: String::from("active"),
		})
	);
}

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
