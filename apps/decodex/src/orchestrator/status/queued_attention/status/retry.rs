use crate::{
	orchestrator::QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT,
	prelude::Result,
	state::{RunActivityMarker, StateStore},
	tracker::TrackerIssue,
};

pub(crate) fn operator_queued_issue_retry_budget_attempts(
	state_store: &StateStore,
	issue: &TrackerIssue,
	marker: Option<&RunActivityMarker>,
) -> Result<i64> {
	let state_retry_attempts = state_store.retry_budget_attempt_count(&issue.id)?;
	let marker_retry_attempts =
		marker.and_then(RunActivityMarker::retry_budget_attempt_count).unwrap_or(0);

	Ok(state_retry_attempts.max(marker_retry_attempts))
}

pub(crate) fn operator_queued_issue_auto_retry_blocked_reason(reason: &str) -> Option<String> {
	match reason {
		"issue_needs_attention" => Some(String::from("needs_attention_label")),
		QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT =>
			Some(String::from(QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT)),
		_ => None,
	}
}
