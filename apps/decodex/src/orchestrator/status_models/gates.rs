use crate::orchestrator::TrackerConnectorBackoff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) enum RetainedCloseoutPrMergeGate {
	Merged,
	NotMerged,
	PullRequestStateReadFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) enum ExternalReviewRequestCiGate {
	Ready,
	WaitForGreenChecks,
	RepairRequired,
}

#[derive(Clone, Copy)]
pub(in crate::orchestrator) enum AccountActivityMode {
	Probe,
	Snapshot,
}

#[derive(Clone, Copy)]
pub(in crate::orchestrator) enum RunIssueMetadataHydration {
	AllRows,
	CurrentLaneRowsOnly,
}

pub(in crate::orchestrator) enum TrackerObserverOutcome {
	Ok,
	Unavailable,
	Backoff(TrackerConnectorBackoff),
}
