use crate::orchestrator::kernel::{
	lane_control::model::LaneControlKernelInput,
	state::{LivenessState, OwnershipState, PolicyState, TerminalizationState},
};

pub(in crate::orchestrator::kernel::lane_control::projection) fn lane_control_next_action(
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
