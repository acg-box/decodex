use crate::orchestrator::kernel::{
	action::OwnedLaneAction,
	command::CommandIntent,
	projection::ProjectionHints,
	reason::ReasonCode,
	state::{LaneStateAxes, PolicyState},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct OwnedLaneDecision {
	pub(in crate::orchestrator) decision_class: OwnedLaneAction,
	pub(in crate::orchestrator) policy_state: PolicyState,
	pub(in crate::orchestrator) lane_state_axes: LaneStateAxes,
	pub(in crate::orchestrator) command_intents: Vec<CommandIntent>,
	pub(in crate::orchestrator) projection_hints: ProjectionHints,
	pub(in crate::orchestrator) blockers: Vec<DecisionBlocker>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct DecisionBlocker {
	pub(in crate::orchestrator) reason: ReasonCode,
	pub(in crate::orchestrator) public_summary: &'static str,
}
impl DecisionBlocker {
	pub(in crate::orchestrator) const fn new(reason: ReasonCode) -> Self {
		Self { reason, public_summary: reason.public_summary() }
	}
}
