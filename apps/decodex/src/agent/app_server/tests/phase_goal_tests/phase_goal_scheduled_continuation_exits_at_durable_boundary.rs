use crate::{
	agent::app_server::{
		PhaseGoalController, PhaseGoalKind, PhaseGoalSpec, PhaseGoalTransition,
		tests::{self, ContinueTokenCompletionHandler},
	},
	prelude::Result,
};

struct ScheduledContinuationController;
impl PhaseGoalController for ScheduledContinuationController {
	fn initial_phase_goal(&self) -> Result<Option<PhaseGoalSpec>> {
		Ok(Some(PhaseGoalSpec::new(
			PhaseGoalKind::ImplementToValidationReady,
			"initial goal",
			None,
		)))
	}

	fn phase_goal_completed(&self, _phase: PhaseGoalKind) -> Result<PhaseGoalTransition> {
		Ok(PhaseGoalTransition::ScheduleContinuation(PhaseGoalSpec::new(
			PhaseGoalKind::RepairValidationFailures,
			"bounded no-effective-delta repair",
			None,
		)))
	}
}

#[test]
fn phase_goal_scheduled_continuation_exits_at_durable_boundary() {
	let handler = ContinueTokenCompletionHandler;
	let controller = ScheduledContinuationController;
	let script = tests::phase_goal_fake_codex_script(&["DONE"], &["complete"], &[]);
	let (result, state_store) = tests::execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 3;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let result = result.expect("scheduled continuation");
	let goal_set_events = tests::private_phase_goal_events(&state_store, "phase_goal_set");

	assert_eq!(result.turn_count, 1);
	assert!(result.continuation_pending);
	assert_eq!(goal_set_events.len(), 2);
	assert_eq!(goal_set_events[1]["phase"], "repair_validation_failures");
}
