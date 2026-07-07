pub(super) const REVIEW_HANDOFF_STATE_DRIFT_DETECTED_EVENT_TYPE: &str =
	"review_handoff_state_drift_detected";
pub(super) const REVIEW_HANDOFF_STATE_DRIFT_RECOVERED_EVENT_TYPE: &str =
	"review_handoff_state_drift_recovered";
pub(super) const REVIEW_HANDOFF_REBOUND_LIFECYCLE_PHASE: &str = "request_pending";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReviewHandoffFailureDriftLineage {
	Exact,
	Descends,
	Diverged,
	Unknown,
}
impl ReviewHandoffFailureDriftLineage {
	pub(super) fn allows_lifecycle_recovery(self) -> bool {
		matches!(self, Self::Exact | Self::Descends)
	}

	pub(super) fn as_str(self) -> &'static str {
		match self {
			Self::Exact => "exact",
			Self::Descends => "descends",
			Self::Diverged => "diverged",
			Self::Unknown => "unknown",
		}
	}
}

pub(super) enum ReviewHandoffStateDriftTransition {
	AlreadySuccess,
	MoveToSuccess(String),
}
