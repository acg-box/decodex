use crate::orchestrator::kernel::{
	lane_control::{model::LaneControlKernelInput, projection::policy},
	state::{LivenessState, OwnershipState, PolicyState, TerminalizationState},
};

pub(in crate::orchestrator::kernel::lane_control::projection) fn lane_control_ownership(
	input: &LaneControlKernelInput<'_>,
	liveness: LivenessState,
	policy: PolicyState,
	terminalization: TerminalizationState,
	needs_attention_signal: bool,
) -> OwnershipState {
	if policy::policy_requires_attention(policy)
		|| needs_attention_signal
		|| !input.run_lease && liveness == LivenessState::HostBootMismatch
	{
		return OwnershipState::RetainedAttention;
	}
	if input.run_lease && input.attempt_active {
		return OwnershipState::LeasedRun;
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
