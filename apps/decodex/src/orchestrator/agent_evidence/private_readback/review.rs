use serde_json::Value;

use crate::orchestrator::PrivateExecutionEvent;
use crate::orchestrator::agent_evidence::{
	PrivateEvidenceReviewCheckpointSummary, PrivateEvidenceReviewRouteCount,
	REVIEW_CHECKPOINT_EVENT_TYPE,
};

pub(super) fn review_checkpoints_from_private_events(
	events: &[PrivateExecutionEvent],
) -> Vec<PrivateEvidenceReviewCheckpointSummary> {
	events
		.iter()
		.filter(|event| event.event_type() == REVIEW_CHECKPOINT_EVENT_TYPE)
		.filter_map(review_checkpoint_from_private_event)
		.collect()
}

fn review_checkpoint_from_private_event(
	event: &PrivateExecutionEvent,
) -> Option<PrivateEvidenceReviewCheckpointSummary> {
	let payload = event.payload();
	let phase = payload.get("phase")?.as_str()?.to_owned();
	let status = payload.get("status")?.as_str()?.to_owned();
	let head_sha = payload.get("head_sha").and_then(Value::as_str).map(str::to_owned);
	let round =
		payload.get("nonclean_rounds").or_else(|| payload.get("round")).and_then(Value::as_u64);
	let (review_class, risk_class, compact_eligible, fallback_reason) =
		review_checkpoint_cost_control_summary(payload);
	let accepted_finding_count = payload
		.get("review")
		.and_then(|review| review.get("accepted_findings"))
		.or_else(|| payload.get("accepted_findings"))
		.and_then(Value::as_array)
		.map_or(0, Vec::len);
	let rejected_finding_count = payload
		.get("review")
		.and_then(|review| review.get("rejected_findings"))
		.or_else(|| payload.get("rejected_findings"))
		.and_then(Value::as_array)
		.map_or(0, Vec::len);
	let (active_fingerprints, stop_fingerprint) = review_checkpoint_fingerprint_summary(payload);
	let (route_counts, route_next_action) = review_checkpoint_route_summary(payload);
	let next_action = review_checkpoint_next_action(&status);

	Some(PrivateEvidenceReviewCheckpointSummary {
		phase,
		status,
		head_sha,
		round,
		review_class,
		risk_class,
		compact_eligible,
		fallback_reason,
		active_fingerprints,
		stop_fingerprint,
		accepted_finding_count,
		rejected_finding_count,
		route_counts,
		route_next_action,
		next_action,
	})
}

fn review_checkpoint_fingerprint_summary(payload: &Value) -> (Vec<String>, Option<String>) {
	let policy = payload.get("review").and_then(|review| review.get("finding_policy"));
	let active_source = payload
		.get("active_fingerprints")
		.or_else(|| policy.and_then(|policy| policy.get("active_fingerprints")));
	let active_fingerprints = active_source
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(str::to_owned)
		.collect();
	let stop_fingerprint = payload
		.get("stop_fingerprint")
		.and_then(Value::as_str)
		.or_else(|| {
			policy.and_then(|policy| policy.get("stop_fingerprint")).and_then(Value::as_str)
		})
		.map(str::to_owned);

	(active_fingerprints, stop_fingerprint)
}

fn review_checkpoint_cost_control_summary(
	payload: &Value,
) -> (Option<String>, Option<String>, Option<bool>, Option<String>) {
	let cost_control = payload.get("review").and_then(|review| review.get("review_cost_control"));
	let review_class = payload
		.get("review_class")
		.and_then(Value::as_str)
		.or_else(|| {
			cost_control
				.and_then(|cost_control| cost_control.get("review_class"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned);
	let risk_class = payload
		.get("risk_class")
		.and_then(Value::as_str)
		.or_else(|| {
			cost_control
				.and_then(|cost_control| cost_control.get("risk_class"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned);
	let compact_eligible = payload.get("compact_eligible").and_then(Value::as_bool).or_else(|| {
		cost_control
			.and_then(|cost_control| cost_control.get("compact_eligible"))
			.and_then(Value::as_bool)
	});
	let fallback_reason = payload
		.get("review_fallback_reason")
		.and_then(Value::as_str)
		.or_else(|| {
			cost_control
				.and_then(|cost_control| cost_control.get("fallback_reason"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned);

	(review_class, risk_class, compact_eligible, fallback_reason)
}

fn review_checkpoint_route_summary(
	payload: &Value,
) -> (Vec<PrivateEvidenceReviewRouteCount>, Option<String>) {
	let review = payload.get("review").unwrap_or(payload);
	let route_summary = review.get("finding_route_summary");
	let route_counts = payload
		.get("route_counts")
		.or_else(|| route_summary.and_then(|summary| summary.get("route_counts")))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|count| {
			Some(PrivateEvidenceReviewRouteCount {
				route: count.get("route")?.as_str()?.to_owned(),
				count: usize::try_from(count.get("count")?.as_u64()?).ok()?,
			})
		})
		.collect();
	let route_next_action = payload
		.get("route_next_action")
		.and_then(Value::as_str)
		.or_else(|| {
			route_summary.and_then(|summary| summary.get("next_action")).and_then(Value::as_str)
		})
		.map(str::to_owned);

	(route_counts, route_next_action)
}

fn review_checkpoint_next_action(status: &str) -> String {
	match status {
		"clean" => String::from("Proceed with review handoff when repo gate evidence is current."),
		"findings" => String::from(
			"Repair accepted findings, rerun validation, and checkpoint the repaired head.",
		),
		"blocked" => String::from("Resolve the blocking review condition before continuing."),
		"needs_architecture_review" => {
			String::from("Escalate for an architecture decision before further repair churn.")
		},
		_ => String::from("Inspect the Decodex Review checkpoint summary before continuing."),
	}
}
