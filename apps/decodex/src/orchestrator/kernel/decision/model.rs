use crate::orchestrator::kernel::{
	action::OwnedLaneAction,
	command::CommandIntent,
	projection::ProjectionHints,
	reason::ReasonCode,
	state::{LaneStateAxes, PolicyState},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OwnedLaneDecision {
	pub(crate) decision_class: OwnedLaneAction,
	pub(crate) policy_state: PolicyState,
	pub(crate) lane_state_axes: LaneStateAxes,
	pub(crate) command_intents: Vec<CommandIntent>,
	pub(crate) projection_hints: ProjectionHints,
	pub(crate) blockers: Vec<DecisionBlocker>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecisionBlocker {
	pub(crate) reason: ReasonCode,
	pub(crate) public_summary: &'static str,
}
impl DecisionBlocker {
	pub(crate) const fn new(reason: ReasonCode) -> Self {
		Self { reason, public_summary: reason.public_summary() }
	}
}
