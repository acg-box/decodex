use crate::orchestrator::{
	OperatorReviewCheckpointStatus, OperatorReviewCheckpointSummaryFields,
	OperatorReviewLoopStatus, OperatorReviewRouteCount, ProjectLoopEvidenceSnapshot, ReviewLevel,
	Value,
};
use crate::prelude::Result;

pub(in crate::orchestrator) fn operator_review_loop_status(
	review_level: ReviewLevel,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_review_phase: Option<&str>,
) -> Result<Option<OperatorReviewLoopStatus>> {
	if let Some(checkpoint) = operator_latest_review_checkpoint_event_status(
		loop_evidence,
		issue_id,
		run_id,
		attempt_number,
	) {
		return Ok(Some(checkpoint));
	}

	let latest_checkpoint = ["handoff", "repair"]
		.into_iter()
		.filter_map(|phase| {
			loop_evidence.review_policy_checkpoint(issue_id, run_id, attempt_number, phase)
		})
		.max_by(|left, right| {
			left.updated_at_unix()
				.cmp(&right.updated_at_unix())
				.then_with(|| left.phase().cmp(right.phase()))
		});

	if let Some(checkpoint) = latest_checkpoint {
		let nonclean_rounds = checkpoint.nonclean_rounds();
		let summary = operator_review_checkpoint_summary_fields(checkpoint.details_json());

		return Ok(Some(OperatorReviewLoopStatus {
			phase: checkpoint.phase().to_owned(),
			status: checkpoint.status().to_owned(),
			checkpoint: Some(OperatorReviewCheckpointStatus {
				head_sha: checkpoint.head_sha().to_owned(),
				round: nonclean_rounds,
				nonclean_rounds,
				review_class: summary.review_class,
				risk_class: summary.risk_class,
				compact_eligible: summary.compact_eligible,
				fallback_reason: summary.fallback_reason,
				active_fingerprints: summary.active_fingerprints,
				stop_fingerprint: summary.stop_fingerprint,
				route_counts: summary.route_counts,
				route_next_action: summary.route_next_action,
				updated_at: checkpoint.updated_at().to_owned(),
			}),
		}));
	}

	if review_level.requires_review_checkpoint()
		&& let Some(default_review_phase) = default_review_phase
	{
		return Ok(Some(OperatorReviewLoopStatus {
			phase: default_review_phase.to_owned(),
			status: String::from("pending"),
			checkpoint: None,
		}));
	}

	Ok(None)
}

pub(in crate::orchestrator) fn operator_latest_review_checkpoint_event_status(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
) -> Option<OperatorReviewLoopStatus> {
	loop_evidence.private_events(issue_id, run_id, attempt_number).iter().rev().find_map(|event| {
		let payload = event.payload();

		if event.event_type() != "review_checkpoint" {
			return None;
		}

		let phase = payload.get("phase").and_then(Value::as_str)?;
		let status = payload.get("status").and_then(Value::as_str)?;
		let head_sha = payload.get("head_sha").and_then(Value::as_str)?;
		let nonclean_rounds = payload.get("nonclean_rounds").and_then(Value::as_i64).unwrap_or(0);
		let checkpoint =
			loop_evidence.review_policy_checkpoint(issue_id, run_id, attempt_number, phase)?;

		if checkpoint.status() != status
			|| checkpoint.head_sha() != head_sha
			|| checkpoint.nonclean_rounds() != nonclean_rounds
		{
			return None;
		}

		let details_json = payload.get("review").unwrap_or(payload).to_string();
		let summary = operator_review_checkpoint_summary_fields(&details_json);

		Some(OperatorReviewLoopStatus {
			phase: phase.to_owned(),
			status: status.to_owned(),
			checkpoint: Some(OperatorReviewCheckpointStatus {
				head_sha: head_sha.to_owned(),
				round: nonclean_rounds,
				nonclean_rounds,
				review_class: summary.review_class,
				risk_class: summary.risk_class,
				compact_eligible: summary.compact_eligible,
				fallback_reason: summary.fallback_reason,
				active_fingerprints: summary.active_fingerprints,
				stop_fingerprint: summary.stop_fingerprint,
				route_counts: summary.route_counts,
				route_next_action: summary.route_next_action,
				updated_at: checkpoint.updated_at().to_owned(),
			}),
		})
	})
}

pub(in crate::orchestrator) fn operator_review_checkpoint_summary_fields(
	details_json: &str,
) -> OperatorReviewCheckpointSummaryFields {
	let Ok(details) = serde_json::from_str::<Value>(details_json) else {
		return OperatorReviewCheckpointSummaryFields {
			review_class: None,
			risk_class: None,
			compact_eligible: None,
			fallback_reason: None,
			active_fingerprints: Vec::new(),
			stop_fingerprint: None,
			route_counts: Vec::new(),
			route_next_action: None,
		};
	};
	let policy = details.get("finding_policy");
	let cost_control = details.get("review_cost_control");
	let review_class = cost_control
		.and_then(|cost_control| cost_control.get("review_class"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let risk_class = cost_control
		.and_then(|cost_control| cost_control.get("risk_class"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let compact_eligible = cost_control
		.and_then(|cost_control| cost_control.get("compact_eligible"))
		.and_then(Value::as_bool);
	let fallback_reason = cost_control
		.and_then(|cost_control| cost_control.get("fallback_reason"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let active_fingerprints = policy
		.and_then(|policy| policy.get("active_fingerprints"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(str::to_owned)
		.collect();
	let stop_fingerprint = policy
		.and_then(|policy| policy.get("stop_fingerprint"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let route_summary = details.get("finding_route_summary");
	let route_counts = route_summary
		.and_then(|summary| summary.get("route_counts"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|count| {
			Some(OperatorReviewRouteCount {
				route: count.get("route")?.as_str()?.to_owned(),
				count: usize::try_from(count.get("count")?.as_u64()?).ok()?,
			})
		})
		.collect();
	let route_next_action = route_summary
		.and_then(|summary| summary.get("next_action"))
		.and_then(Value::as_str)
		.map(str::to_owned);

	OperatorReviewCheckpointSummaryFields {
		review_class,
		risk_class,
		compact_eligible,
		fallback_reason,
		active_fingerprints,
		stop_fingerprint,
		route_counts,
		route_next_action,
	}
}
