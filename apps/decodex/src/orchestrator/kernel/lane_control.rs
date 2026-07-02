use super::state::{
	LaneStateAxes, LivenessState, OwnershipState, PolicyState, TerminalizationState,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct LaneControlKernelInput<'a> {
	pub(in crate::orchestrator) run_lease: bool,
	pub(in crate::orchestrator) attempt_active: bool,
	pub(in crate::orchestrator) attempt_terminal: bool,
	pub(in crate::orchestrator) status_starting_or_running: bool,
	pub(in crate::orchestrator) status_needs_attention_or_terminal_failure: bool,
	pub(in crate::orchestrator) phase_executing: bool,
	pub(in crate::orchestrator) phase_needs_attention: bool,
	pub(in crate::orchestrator) phase_stalled: bool,
	pub(in crate::orchestrator) phase_terminal: bool,
	pub(in crate::orchestrator) cleanup_complete_signal: bool,
	pub(in crate::orchestrator) current_operation_ledger_outcome: bool,
	pub(in crate::orchestrator) wait_reason_present: bool,
	pub(in crate::orchestrator) continuation_wait: bool,
	pub(in crate::orchestrator) process_alive: Option<bool>,
	pub(in crate::orchestrator) execution_liveness_observed: bool,
	pub(in crate::orchestrator) host_boot_mismatch: bool,
	pub(in crate::orchestrator) not_running_signal: bool,
	pub(in crate::orchestrator) thread_active: bool,
	pub(in crate::orchestrator) protocol_recent: bool,
	pub(in crate::orchestrator) suspected_stall: bool,
	pub(in crate::orchestrator) stale_execution_without_known_process: bool,
	pub(in crate::orchestrator) policy: PolicyState,
	pub(in crate::orchestrator) loop_next_action: Option<&'a str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct LaneControlKernelProjection {
	pub(in crate::orchestrator) axes: LaneStateAxes,
	pub(in crate::orchestrator) next_action: String,
	pub(in crate::orchestrator) conditions: Vec<&'static str>,
	pub(in crate::orchestrator) has_fresh_execution: bool,
	pub(in crate::orchestrator) has_live_execution: bool,
	pub(in crate::orchestrator) has_authoritative_live_owner: bool,
	pub(in crate::orchestrator) needs_attention_signal: bool,
	pub(in crate::orchestrator) counts_as_attention: bool,
	pub(in crate::orchestrator) counts_as_running: bool,
	pub(in crate::orchestrator) counts_as_current_lane: bool,
}

pub(in crate::orchestrator) fn project_lane_control(
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

#[cfg(test)]
mod tests {
	use super::*;

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
		let projection = project_lane_control(&input());

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

		let projection = project_lane_control(&input);

		assert_eq!(projection.axes.ownership, OwnershipState::OrphanedLiveThread);
		assert_eq!(projection.next_action, "inspect_or_interrupt_orphaned_live_thread");
		assert!(projection.conditions.contains(&"run_lease_missing"));
		assert!(projection.counts_as_current_lane);
	}

	#[test]
	fn continuation_recovery_churn_requires_attention() {
		let mut input = input();
		input.policy = PolicyState::ContinuationRecoveryChurnExceeded;

		let projection = project_lane_control(&input);

		assert_eq!(projection.axes.ownership, OwnershipState::RetainedAttention);
		assert_eq!(
			projection.next_action,
			"stop_auto_continuation_and_request_architecture_recovery"
		);
		assert!(projection.counts_as_attention);
	}

	#[test]
	fn terminal_phase_hides_current_lane_even_with_live_execution() {
		let mut input = input();
		input.phase_executing = false;
		input.phase_terminal = true;

		let projection = project_lane_control(&input);

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

		let projection = project_lane_control(&input);

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

		let projection = project_lane_control(&input);

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

		let projection = project_lane_control(&input);

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

		let projection = project_lane_control(&input);

		assert_eq!(projection.axes.liveness, LivenessState::LateProtocolActivity);
		assert_eq!(projection.axes.terminalization, TerminalizationState::BarrierStarted);
		assert_eq!(projection.next_action, "finish_terminalization");
		assert!(!projection.counts_as_current_lane);
	}
}
