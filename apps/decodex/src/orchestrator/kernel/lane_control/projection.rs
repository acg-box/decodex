use crate::orchestrator::kernel::{
	lane_control::model::{LaneControlKernelInput, LaneControlKernelProjection},
	state::{LaneStateAxes, LivenessState, OwnershipState, PolicyState, TerminalizationState},
};

pub(crate) fn project_lane_control(
	input: &LaneControlKernelInput<'_>,
) -> LaneControlKernelProjection {
	let has_fresh_execution = input.status_starting_or_running
		&& (input.process_alive == Some(true) || input.protocol_recent);
	let has_live_execution = input.status_starting_or_running
		&& (input.execution_liveness_observed
			|| input.process_alive == Some(true)
			|| input.thread_active
			|| input.protocol_recent);
	let has_authoritative_live_owner =
		input.run_lease || input.process_alive == Some(true) || input.thread_active;
	let needs_attention_signal = input.status_needs_attention_or_terminal_failure
		|| input.phase_needs_attention
		|| input.suspected_stall
		|| input.phase_stalled
		|| input.process_alive == Some(false) && input.status_starting_or_running
		|| input.stale_execution_without_known_process;
	let liveness = lane_control_liveness(input);
	let terminalization = lane_control_terminalization(input, liveness);
	let ownership = lane_control_ownership(
		input,
		liveness,
		input.policy,
		terminalization,
		needs_attention_signal,
	);
	let next_action =
		lane_control_next_action(input, ownership, liveness, input.policy, terminalization);
	let conditions = lane_control_conditions(input, ownership, liveness, input.policy);
	let counts_as_attention = needs_attention_signal
		|| ownership == OwnershipState::RetainedAttention
		|| policy_requires_attention(input.policy);
	let counts_as_running = ownership == OwnershipState::LeasedRun
		&& input.status_starting_or_running
		&& input.phase_executing
		&& input.process_alive != Some(false)
		&& !needs_attention_signal;
	let counts_as_current_lane = (input.run_lease || has_live_execution) && !input.phase_terminal;

	LaneControlKernelProjection {
		axes: LaneStateAxes::new(ownership, liveness, input.policy, terminalization),
		next_action,
		conditions,
		has_fresh_execution,
		has_live_execution,
		has_authoritative_live_owner,
		needs_attention_signal,
		counts_as_attention,
		counts_as_running,
		counts_as_current_lane,
	}
}

fn lane_control_liveness(input: &LaneControlKernelInput<'_>) -> LivenessState {
	if input.host_boot_mismatch {
		return LivenessState::HostBootMismatch;
	}
	if input.process_alive == Some(true) {
		return LivenessState::ProcessAlive;
	}
	if input.process_alive == Some(false) || input.not_running_signal {
		return LivenessState::NotRunning;
	}
	if input.thread_active {
		return LivenessState::ThreadActive;
	}
	if input.phase_terminal && input.protocol_recent {
		return LivenessState::LateProtocolActivity;
	}
	if input.protocol_recent {
		return LivenessState::ProtocolRecent;
	}

	LivenessState::Unknown
}

fn lane_control_terminalization(
	input: &LaneControlKernelInput<'_>,
	liveness: LivenessState,
) -> TerminalizationState {
	if input.cleanup_complete_signal
		|| input.current_operation_ledger_outcome && input.phase_terminal
	{
		return TerminalizationState::CleanupComplete;
	}
	if input.phase_terminal
		&& !input.run_lease
		&& matches!(liveness, LivenessState::NotRunning | LivenessState::Unknown)
	{
		return TerminalizationState::CleanupComplete;
	}
	if input.phase_terminal && liveness == LivenessState::LateProtocolActivity && !input.run_lease {
		return TerminalizationState::CleanupComplete;
	}
	if input.phase_terminal {
		return TerminalizationState::BarrierStarted;
	}

	TerminalizationState::None
}

fn lane_control_ownership(
	input: &LaneControlKernelInput<'_>,
	liveness: LivenessState,
	policy: PolicyState,
	terminalization: TerminalizationState,
	needs_attention_signal: bool,
) -> OwnershipState {
	if input.run_lease && input.attempt_active && !policy_requires_attention(policy) {
		return OwnershipState::LeasedRun;
	}
	if policy_requires_attention(policy)
		|| needs_attention_signal
		|| !input.run_lease && liveness == LivenessState::HostBootMismatch
	{
		return OwnershipState::RetainedAttention;
	}
	if input.continuation_wait {
		return OwnershipState::ContinuationPending;
	}
	if !input.run_lease
		&& matches!(
			liveness,
			LivenessState::ProcessAlive
				| LivenessState::ThreadActive
				| LivenessState::ProtocolRecent
		) {
		return OwnershipState::OrphanedLiveThread;
	}
	if terminalization != TerminalizationState::None
		&& terminalization != TerminalizationState::CleanupComplete
	{
		return OwnershipState::Terminalizing;
	}
	if input.attempt_active {
		return OwnershipState::Pending;
	}

	OwnershipState::Closed
}

fn lane_control_conditions(
	input: &LaneControlKernelInput<'_>,
	ownership: OwnershipState,
	liveness: LivenessState,
	policy: PolicyState,
) -> Vec<&'static str> {
	let mut conditions = Vec::new();

	if !input.run_lease && input.attempt_active {
		conditions.push("run_lease_missing");
	}
	if input.attempt_terminal
		&& matches!(
			liveness,
			LivenessState::ProcessAlive
				| LivenessState::ThreadActive
				| LivenessState::ProtocolRecent
		) {
		conditions.push("terminal_attempt_has_live_evidence");
	}
	if liveness == LivenessState::HostBootMismatch {
		conditions.push("host_boot_id_mismatch");
	}
	if policy == PolicyState::ReviewChurnExceeded {
		conditions.push("review_churn_threshold_exceeded");
	}
	if policy == PolicyState::ContinuationRecoveryChurnExceeded {
		conditions.push("continuation_recovery_budget_exceeded");
	}
	if liveness == LivenessState::LateProtocolActivity {
		conditions.push("late_protocol_activity_after_terminal_barrier");
	}
	if matches!(
		policy,
		PolicyState::AuthorityBoundaryRequired | PolicyState::HumanAttentionRequired
	) {
		conditions.push("policy_requires_human_attention");
	}
	if ownership == OwnershipState::LeasedRun && !input.run_lease {
		conditions.push("invalid_leased_run_without_lease");
	}

	conditions
}

fn lane_control_next_action(
	input: &LaneControlKernelInput<'_>,
	ownership: OwnershipState,
	liveness: LivenessState,
	policy: PolicyState,
	terminalization: TerminalizationState,
) -> String {
	match policy {
		PolicyState::ReviewChurnExceeded => {
			return String::from("start_architecture_recovery_or_stop_for_human_attention");
		},
		PolicyState::ContinuationRecoveryChurnExceeded => {
			return String::from("stop_auto_continuation_and_request_architecture_recovery");
		},
		PolicyState::AuthorityBoundaryRequired | PolicyState::HumanAttentionRequired => {
			return String::from("resolve_policy_stop_before_mutating_lane");
		},
		_ => {},
	}

	if ownership == OwnershipState::OrphanedLiveThread {
		return String::from("inspect_or_interrupt_orphaned_live_thread");
	}
	if liveness == LivenessState::HostBootMismatch {
		return String::from("inspect_recovery_evidence");
	}
	if terminalization != TerminalizationState::None
		&& terminalization != TerminalizationState::CleanupComplete
	{
		return String::from("finish_terminalization");
	}
	if liveness == LivenessState::LateProtocolActivity {
		return String::from("ignore_late_activity");
	}
	if ownership == OwnershipState::LeasedRun {
		return input
			.loop_next_action
			.map(str::to_owned)
			.unwrap_or_else(|| String::from("continue_owned_attempt"));
	}
	if ownership == OwnershipState::ContinuationPending {
		return String::from("wait_for_continuation_reentry");
	}
	if ownership == OwnershipState::Closed {
		return String::from("no_action");
	}

	input.loop_next_action.map(str::to_owned).unwrap_or_else(|| String::from("inspect_lane_state"))
}

fn policy_requires_attention(policy: PolicyState) -> bool {
	matches!(
		policy,
		PolicyState::ReviewChurnExceeded
			| PolicyState::ContinuationRecoveryChurnExceeded
			| PolicyState::AuthorityBoundaryRequired
			| PolicyState::HumanAttentionRequired
	)
}
