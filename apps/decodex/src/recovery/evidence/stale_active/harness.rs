use crate::{recovery::evidence::json, state::PrivateExecutionEvent};

pub(in crate::recovery::evidence::stale_active) fn stale_active_event_is_no_progress_harness(
	event: &PrivateExecutionEvent,
) -> bool {
	if event.event_type() != "harness_outcome" {
		return false;
	}

	let payload = event.payload();

	payload.get("schema").and_then(serde_json::Value::as_str) == Some("decodex.harness_outcome/1")
		&& payload.pointer("/source/outcome").and_then(serde_json::Value::as_str)
			== Some("retryable_failure")
		&& payload.pointer("/pr_lifecycle/outcome").and_then(serde_json::Value::as_str)
			== Some("retryable_failure")
		&& payload.pointer("/manual_attention").is_none_or(serde_json::Value::is_null)
		&& json::array_is_missing_or_empty(payload.get("contracts"))
		&& json::array_is_missing_or_empty(payload.get("execution_programs"))
		&& stale_active_harness_pr_lifecycle_has_no_progress(payload)
		&& stale_active_harness_review_has_no_progress(payload)
		&& stale_active_harness_validation_has_no_progress(payload)
}

fn stale_active_harness_pr_lifecycle_has_no_progress(payload: &serde_json::Value) -> bool {
	json::array_is_missing_or_empty(payload.pointer("/pr_lifecycle/pr_urls"))
}

fn stale_active_harness_review_has_no_progress(payload: &serde_json::Value) -> bool {
	let review = payload.pointer("/review");
	let statuses = review.and_then(|review| review.get("statuses"));
	let accepted_findings = review.and_then(|review| review.get("accepted_finding_count"));
	let rejected_findings = review.and_then(|review| review.get("rejected_finding_count"));
	let nonclean_rounds = review.and_then(|review| review.get("nonclean_rounds"));

	json::array_is_missing_or_empty(statuses)
		&& json::number_is_zero_or_missing(accepted_findings)
		&& json::number_is_zero_or_missing(rejected_findings)
		&& json::number_is_zero_or_missing(nonclean_rounds)
}

fn stale_active_harness_validation_has_no_progress(payload: &serde_json::Value) -> bool {
	let validation = payload.pointer("/validation");
	let validation_result = validation
		.and_then(|validation| validation.get("result"))
		.and_then(serde_json::Value::as_str);
	let failure_count = validation.and_then(|validation| validation.get("failure_count"));
	let failure_classes = validation.and_then(|validation| validation.get("failure_classes"));

	validation_result.is_none_or(|result| result == "not_recorded")
		&& json::number_is_zero_or_missing(failure_count)
		&& json::array_is_missing_or_empty(failure_classes)
}
