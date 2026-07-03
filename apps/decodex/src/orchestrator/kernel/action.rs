#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OwnedLaneAction {
	Continue,
	WaitForExternalSignal,
	RetryAutomatically,
	ResumeRetainedLane,
	ManualInterventionRequired,
	ReadyToLand,
}
impl OwnedLaneAction {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Continue => "continue",
			Self::WaitForExternalSignal => "wait_for_external_signal",
			Self::RetryAutomatically => "retry_automatically",
			Self::ResumeRetainedLane => "resume_retained_lane",
			Self::ManualInterventionRequired => "manual_intervention_required",
			Self::ReadyToLand => "ready_to_land",
		}
	}
}
