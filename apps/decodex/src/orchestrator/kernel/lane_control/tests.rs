use crate::orchestrator::kernel::{
	lane_control::{self, LaneControlKernelInput},
	state::{LivenessState, OwnershipState, PolicyState, TerminalizationState},
};

fn input() -> LaneControlKernelInput<'static> {
	LaneControlKernelInput {
		run_lease: true,
		attempt_active: true,
		attempt_terminal: false,
		status_starting_or_running: true,
		status_needs_attention_or_terminal_failure: false,
		phase_executing: true,
		phase_needs_attention: false,
		phase_stalled: false,
		phase_terminal: false,
		cleanup_complete_signal: false,
		current_operation_ledger_outcome: false,
		wait_reason_present: false,
		continuation_wait: false,
		process_alive: Some(true),
		execution_liveness_observed: true,
		host_boot_mismatch: false,
		not_running_signal: false,
		thread_active: false,
		protocol_recent: false,
		suspected_stall: false,
		stale_execution_without_known_process: false,
		policy: PolicyState::Allowed,
		loop_next_action: None,
	}
}

#[test]
fn leased_running_lane_projects_stable_status_fields() {
	let projection = lane_control::project_lane_control(&input());

	assert_eq!(projection.axes.ownership, OwnershipState::LeasedRun);
	assert_eq!(projection.axes.liveness, LivenessState::ProcessAlive);
	assert_eq!(projection.axes.policy, PolicyState::Allowed);
	assert_eq!(projection.next_action, "continue_owned_attempt");
	assert!(projection.counts_as_running);
	assert!(projection.counts_as_current_lane);
	assert!(projection.has_live_execution);
	assert!(projection.has_authoritative_live_owner);
}

#[test]
fn orphaned_live_thread_is_attention_owned_projection() {
	let mut input = input();

	input.run_lease = false;

	let projection = lane_control::project_lane_control(&input);

	assert_eq!(projection.axes.ownership, OwnershipState::OrphanedLiveThread);
	assert_eq!(projection.next_action, "inspect_or_interrupt_orphaned_live_thread");
	assert!(projection.conditions.contains(&"run_lease_missing"));
	assert!(projection.counts_as_current_lane);
}

#[test]
fn continuation_recovery_churn_requires_attention() {
	let mut input = input();

	input.policy = PolicyState::ContinuationRecoveryChurnExceeded;

	let projection = lane_control::project_lane_control(&input);

	assert_eq!(projection.axes.ownership, OwnershipState::RetainedAttention);
	assert_eq!(projection.next_action, "stop_auto_continuation_and_request_architecture_recovery");
	assert!(projection.counts_as_attention);
}

#[test]
fn terminal_phase_hides_current_lane_even_with_live_execution() {
	let mut input = input();

	input.phase_executing = false;
	input.phase_terminal = true;

	let projection = lane_control::project_lane_control(&input);

	assert!(projection.has_live_execution);
	assert!(!projection.counts_as_current_lane);
}

#[test]
fn serialized_liveness_observation_preserves_live_execution_readback() {
	let mut input = input();

	input.run_lease = false;
	input.process_alive = None;
	input.execution_liveness_observed = true;
	input.thread_active = false;
	input.protocol_recent = false;

	let projection = lane_control::project_lane_control(&input);

	assert_eq!(projection.axes.liveness, LivenessState::Unknown);
	assert!(projection.has_live_execution);
	assert!(!projection.has_authoritative_live_owner);
	assert!(projection.counts_as_current_lane);
}

#[test]
fn active_thread_is_authoritative_live_owner_without_lease() {
	let mut input = input();

	input.run_lease = false;
	input.process_alive = None;
	input.execution_liveness_observed = false;
	input.thread_active = true;
	input.protocol_recent = false;

	let projection = lane_control::project_lane_control(&input);

	assert!(projection.has_authoritative_live_owner);
	assert!(projection.has_live_execution);
}

#[test]
fn late_protocol_after_terminal_barrier_stays_closed() {
	let mut input = input();

	input.run_lease = false;
	input.attempt_active = false;
	input.attempt_terminal = true;
	input.status_starting_or_running = false;
	input.phase_executing = false;
	input.phase_terminal = true;
	input.process_alive = None;
	input.execution_liveness_observed = true;
	input.protocol_recent = true;

	let projection = lane_control::project_lane_control(&input);

	assert_eq!(projection.axes.ownership, OwnershipState::Closed);
	assert_eq!(projection.axes.liveness, LivenessState::LateProtocolActivity);
	assert_eq!(projection.axes.terminalization, TerminalizationState::CleanupComplete);
	assert_eq!(projection.next_action, "ignore_late_activity");
	assert!(projection.conditions.contains(&"late_protocol_activity_after_terminal_barrier"));
	assert!(!projection.counts_as_current_lane);
}

#[test]
fn late_protocol_does_not_mask_terminalization_work() {
	let mut input = input();

	input.attempt_terminal = true;
	input.status_starting_or_running = false;
	input.phase_executing = false;
	input.phase_terminal = true;
	input.process_alive = None;
	input.execution_liveness_observed = true;
	input.protocol_recent = true;

	let projection = lane_control::project_lane_control(&input);

	assert_eq!(projection.axes.liveness, LivenessState::LateProtocolActivity);
	assert_eq!(projection.axes.terminalization, TerminalizationState::BarrierStarted);
	assert_eq!(projection.next_action, "finish_terminalization");
	assert!(!projection.counts_as_current_lane);
}
