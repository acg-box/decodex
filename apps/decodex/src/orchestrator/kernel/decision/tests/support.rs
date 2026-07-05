use crate::orchestrator::kernel::{
	facts::LaneObservation,
	state::{LaneStateAxes, LivenessState, OwnershipState, PolicyState, TerminalizationState},
};

pub(crate) fn observation() -> LaneObservation {
	LaneObservation::for_issue("PUB-101")
}

pub(crate) fn authoritative_observation() -> LaneObservation {
	let mut observation = observation();

	observation.authority_complete = true;

	observation
}

pub(crate) fn axes(
	ownership: OwnershipState,
	liveness: LivenessState,
	policy: PolicyState,
	terminalization: TerminalizationState,
) -> LaneStateAxes {
	LaneStateAxes::new(ownership, liveness, policy, terminalization)
}
