use crate::orchestrator::OperatorQueuedIssueStatus;

pub(super) fn queued_candidate_counts_as_waiting_intake(
	candidate: &OperatorQueuedIssueStatus,
) -> bool {
	!matches!(candidate.classification.as_str(), "claimed" | "closed")
}

pub(super) fn queued_candidate_counts_as_attention(candidate: &OperatorQueuedIssueStatus) -> bool {
	candidate.classification == "blocked" || candidate.attention.is_some()
}
