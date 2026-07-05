use crate::orchestrator::kernel::{
	lane_control::model::LaneControlKernelInput, state::LivenessState,
};

pub(in crate::orchestrator::kernel::lane_control::projection) fn lane_control_liveness(
	input: &LaneControlKernelInput<'_>,
) -> LivenessState {
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
