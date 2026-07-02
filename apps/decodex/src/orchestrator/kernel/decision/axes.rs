use crate::orchestrator::kernel::{
	facts::LaneObservation,
	state::{LaneStateAxes, OwnershipState, PolicyState, TerminalizationState},
};

pub(super) fn lane_state_axes(
	observation: &LaneObservation,
	policy_state: PolicyState,
) -> LaneStateAxes {
	let ownership = if policy_state == PolicyState::HumanAttentionRequired {
		OwnershipState::RetainedAttention
	} else if observation.terminalization != TerminalizationState::None {
		OwnershipState::Terminalizing
	} else if observation.run_lease && observation.active_owned_work {
		OwnershipState::LeasedRun
	} else if !observation.run_lease && observation.active_owned_work {
		OwnershipState::OrphanedLiveThread
	} else {
		OwnershipState::Pending
	};

	LaneStateAxes::new(ownership, observation.liveness, policy_state, observation.terminalization)
}
