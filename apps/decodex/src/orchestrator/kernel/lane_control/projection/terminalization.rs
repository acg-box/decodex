use crate::orchestrator::kernel::{
	lane_control::model::LaneControlKernelInput,
	state::{LivenessState, TerminalizationState},
};

pub(in crate::orchestrator::kernel::lane_control::projection) fn lane_control_terminalization(
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
