use crate::orchestrator::{
	GHOST_LANE_NEXT_ACTION, GHOST_LANE_OWNERSHIP_STATE, GHOST_LANE_POLICY_STATE,
	GHOST_LANE_TERMINAL_STATUS, OperatorRunStatus,
	kernel::state::{LivenessState, OwnershipState, PolicyState, TerminalizationState},
};

pub(crate) fn missing_issue_ghost_lane_status_allows_cleanup(run: &OperatorRunStatus) -> bool {
	run.ownership_state == GHOST_LANE_OWNERSHIP_STATE
		&& run.policy_state == GHOST_LANE_POLICY_STATE
		&& run.lane_control_next_action == GHOST_LANE_NEXT_ACTION
}

pub(crate) fn missing_issue_ghost_lane_status_is_cleanup_complete(run: &OperatorRunStatus) -> bool {
	run.ownership_state == OwnershipState::Closed.as_str()
		&& run.policy_state == PolicyState::Allowed.as_str()
		&& run.lane_control_next_action == "no_action"
		&& missing_issue_ghost_lane_cleanup_audit_present(run)
}

pub(crate) fn missing_issue_ghost_lane_cleanup_audit_present(run: &OperatorRunStatus) -> bool {
	run.lane_control_conditions
		.iter()
		.any(|condition| condition == "ghost_lane_cleanup_audit_present")
}

pub(crate) fn apply_missing_issue_cleanup_projection(run: &mut OperatorRunStatus) {
	run.status = String::from(GHOST_LANE_TERMINAL_STATUS);
	run.attempt_status = String::from(GHOST_LANE_TERMINAL_STATUS);
	run.status_projection_reason = None;
	run.ownership_state = String::from(OwnershipState::Closed.as_str());
	run.liveness_state = String::from(LivenessState::NotRunning.as_str());
	run.policy_state = String::from(PolicyState::Allowed.as_str());
	run.terminalization_state = String::from(TerminalizationState::CleanupComplete.as_str());
	run.lane_control_next_action = String::from("no_action");
	run.phase = String::from("completed");
	run.run_phase = String::from("completed");
	run.wait_reason = None;
	run.current_operation = String::from("ghost_lane_cleanup_audit");
	run.control_capability = None;
	run.continuation_pending = false;
	run.run_lease = false;
	run.queue_lease_state = String::from("not_held");
	run.execution_liveness = String::from("not_running");
	run.has_fresh_execution = false;
	run.counts_as_running = false;
	run.needs_attention = false;
	run.suspected_stall = false;
	run.retry_kind = None;
	run.next_retry_at = None;

	if let Some(loop_status) = run.loop_status.as_mut() {
		loop_status.summary = String::from("missing-issue ghost cleanup audit recorded");
		loop_status.next_action = None;
		loop_status.review = None;
	}
}

pub(crate) fn append_lane_control_condition(run: &mut OperatorRunStatus, condition: &str) {
	if !run.lane_control_conditions.iter().any(|value| value == condition) {
		run.lane_control_conditions.push(condition.to_owned());
	}
}
