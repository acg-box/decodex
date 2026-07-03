use crate::orchestrator::kernel::{action::OwnedLaneAction, reason::ReasonCode};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProjectionHints {
	pub(crate) lane_control_next_action: &'static str,
	pub(crate) primary_reason: ReasonCode,
}
impl ProjectionHints {
	pub(crate) const fn new(
		action: OwnedLaneAction,
		lane_control_next_action: &'static str,
		primary_reason: ReasonCode,
	) -> Self {
		let _ = action;

		Self { lane_control_next_action, primary_reason }
	}
}
