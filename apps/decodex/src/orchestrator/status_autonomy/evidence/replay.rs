use std::collections::BTreeSet;

use serde_json::Value;

use crate::{
	orchestrator::{OperatorAutonomyExecutionEvidenceStatus, status_autonomy},
	state::{PrivateExecutionEvent, ReviewLifecycleRecord},
};

const AUTONOMY_REPLAY_EVIDENCE_SCHEMA: &str = "decodex.autonomy_replay_evidence/1";

pub(crate) fn operator_autonomy_evidence_completeness_rank(value: &str) -> u8 {
	match value {
		"complete" => 1,
		_ => 0,
	}
}

pub(crate) fn operator_autonomy_replay_evidence_status_from_event(
	event: &PrivateExecutionEvent,
	proposal_id: &str,
	contract_ids: &BTreeSet<&str>,
	issue_identifier: Option<&str>,
	review_lifecycle_records: &[&ReviewLifecycleRecord],
) -> Option<OperatorAutonomyExecutionEvidenceStatus> {
	let payload = event.payload();

	if payload.get("schema").and_then(Value::as_str) != Some(AUTONOMY_REPLAY_EVIDENCE_SCHEMA) {
		return None;
	}
	if !operator_autonomy_replay_evidence_matches(payload, proposal_id, contract_ids) {
		return None;
	}

	let kind = match payload.get("kind").and_then(Value::as_str) {
		Some(kind @ ("pr" | "validation" | "post_land")) => kind.to_owned(),
		_ => return None,
	};
	let raw_source_refs = payload
		.get("source_refs")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(str::to_owned)
		.collect::<Vec<_>>();
	let (source_refs, refs_redacted) = status_autonomy::public_autonomy_refs(&raw_source_refs);
	let (summary, summary_redacted) = status_autonomy::public_status_value(
		payload
			.get("summary")
			.and_then(Value::as_str)
			.unwrap_or("Dogfood replay evidence recorded."),
	);
	let mut known_gaps = Vec::new();

	if kind == "pr" {
		let pr_head_ref = payload
			.get("pr_head_ref")
			.and_then(Value::as_str)
			.map(str::trim)
			.filter(|value| !value.is_empty());
		let pr_head_oid = payload
			.get("pr_head_oid")
			.and_then(Value::as_str)
			.map(str::trim)
			.filter(|value| !value.is_empty());

		return Some(
			match operator_autonomy_matching_pr_review(
				event,
				&raw_source_refs,
				pr_head_ref,
				pr_head_oid,
				review_lifecycle_records,
			) {
				Some(review) => operator_autonomy_pr_evidence_status_from_event(
					event,
					review,
					issue_identifier,
					summary,
					summary_redacted,
				),
				None => {
					if source_refs.is_empty() {
						known_gaps.push(String::from("source_refs_missing_or_redacted"));
					}
					if refs_redacted {
						known_gaps.push(String::from("source_refs_redacted"));
					}
					if summary_redacted {
						known_gaps.push(String::from("summary_redacted"));
					}
					if pr_head_ref.is_none() || pr_head_oid.is_none() {
						known_gaps.push(String::from("pr_head_identity_missing"));
					} else if operator_autonomy_pr_review_candidate_exists(
						event,
						&raw_source_refs,
						review_lifecycle_records,
					) {
						known_gaps.push(String::from("review_lifecycle_stale_or_mismatched"));
					} else {
						known_gaps.push(String::from("review_lifecycle_missing"));
					}

					OperatorAutonomyExecutionEvidenceStatus {
						kind,
						issue_identifier: issue_identifier.map(str::to_owned),
						source_refs,
						summary,
						updated_at: event.recorded_at().to_owned(),
						completeness: status_autonomy::operator_autonomy_completeness(&known_gaps),
						known_gaps,
					}
				},
			},
		);
	}
	if source_refs.is_empty() {
		known_gaps.push(String::from("source_refs_missing_or_redacted"));
	}
	if refs_redacted {
		known_gaps.push(String::from("source_refs_redacted"));
	}
	if summary_redacted {
		known_gaps.push(String::from("summary_redacted"));
	}

	Some(OperatorAutonomyExecutionEvidenceStatus {
		kind,
		issue_identifier: issue_identifier.map(str::to_owned),
		source_refs,
		summary,
		updated_at: event.recorded_at().to_owned(),
		completeness: status_autonomy::operator_autonomy_completeness(&known_gaps),
		known_gaps,
	})
}

fn operator_autonomy_pr_evidence_status_from_event(
	event: &PrivateExecutionEvent,
	review: &ReviewLifecycleRecord,
	issue_identifier: Option<&str>,
	summary: String,
	summary_redacted: bool,
) -> OperatorAutonomyExecutionEvidenceStatus {
	let (source_refs, refs_redacted) =
		status_autonomy::public_autonomy_refs(&[review.pr_url().to_owned()]);
	let mut known_gaps = Vec::new();

	if source_refs.is_empty() {
		known_gaps.push(String::from("source_refs_missing_or_redacted"));
	}
	if refs_redacted {
		known_gaps.push(String::from("source_refs_redacted"));
	}
	if summary_redacted {
		known_gaps.push(String::from("summary_redacted"));
	}

	OperatorAutonomyExecutionEvidenceStatus {
		kind: String::from("pr"),
		issue_identifier: issue_identifier.map(str::to_owned),
		source_refs,
		summary,
		updated_at: [review.updated_at(), event.recorded_at()]
			.into_iter()
			.max()
			.unwrap_or_else(|| event.recorded_at())
			.to_owned(),
		completeness: status_autonomy::operator_autonomy_completeness(&known_gaps),
		known_gaps,
	}
}

fn operator_autonomy_matching_pr_review<'a>(
	event: &PrivateExecutionEvent,
	raw_source_refs: &[String],
	pr_head_ref: Option<&str>,
	pr_head_oid: Option<&str>,
	review_lifecycle_records: &'a [&'a ReviewLifecycleRecord],
) -> Option<&'a ReviewLifecycleRecord> {
	let pr_head_ref = pr_head_ref?;
	let pr_head_oid = pr_head_oid?;
	let raw_source_refs =
		raw_source_refs.iter().map(|source_ref| source_ref.trim()).collect::<BTreeSet<_>>();

	review_lifecycle_records
		.iter()
		.copied()
		.filter(|review| {
			review.run_id() == event.run_id()
				&& review.attempt_number() == event.attempt_number()
				&& raw_source_refs.contains(review.pr_url())
				&& review.branch_name() == pr_head_ref
				&& review.pr_head_ref_name() == pr_head_ref
				&& review.pr_head_oid() == pr_head_oid
				&& review.head_sha() == pr_head_oid
		})
		.max_by(|left, right| {
			left.updated_at_unix()
				.cmp(&right.updated_at_unix())
				.then_with(|| left.branch_name().cmp(right.branch_name()))
		})
}

fn operator_autonomy_pr_review_candidate_exists(
	event: &PrivateExecutionEvent,
	raw_source_refs: &[String],
	review_lifecycle_records: &[&ReviewLifecycleRecord],
) -> bool {
	let raw_source_refs =
		raw_source_refs.iter().map(|source_ref| source_ref.trim()).collect::<BTreeSet<_>>();

	review_lifecycle_records.iter().any(|review| {
		review.run_id() == event.run_id()
			&& review.attempt_number() == event.attempt_number()
			&& raw_source_refs.contains(review.pr_url())
	})
}

fn operator_autonomy_replay_evidence_matches(
	payload: &Value,
	proposal_id: &str,
	contract_ids: &BTreeSet<&str>,
) -> bool {
	payload.get("proposal_id").and_then(Value::as_str) == Some(proposal_id)
		|| payload
			.get("contract_id")
			.and_then(Value::as_str)
			.is_some_and(|contract_id| contract_ids.contains(contract_id))
}
