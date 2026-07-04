mod hard;
mod policy;
mod soft;

pub(super) use self::{
	hard::attempt_hard_lane_interrupt,
	policy::{lane_interrupt_next_action, soft_interrupt_allows_hard_fallback},
	soft::attempt_soft_lane_interrupt,
};
