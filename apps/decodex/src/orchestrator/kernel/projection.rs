use super::{action::OwnedLaneAction, reason::ReasonCode};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct ProjectionHints {
	pub(in crate::orchestrator) lane_control_next_action: &'static str,
	pub(in crate::orchestrator) primary_reason: ReasonCode,
}

impl ProjectionHints {
	pub(in crate::orchestrator) const fn new(
		action: OwnedLaneAction,
		lane_control_next_action: &'static str,
		primary_reason: ReasonCode,
	) -> Self {
		let _ = action;

		Self { lane_control_next_action, primary_reason }
	}
}
