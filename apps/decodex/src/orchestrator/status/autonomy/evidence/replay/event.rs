use std::collections::BTreeSet;

use serde_json::Value;

use crate::{
	orchestrator::{
		OperatorAutonomyExecutionEvidenceStatus, status_autonomy,
		status_autonomy::evidence::replay::{matching, pr_status},
	},
	state::{PrivateExecutionEvent, ReviewLifecycleRecord},
};

const AUTONOMY_REPLAY_EVIDENCE_SCHEMA: &str = "decodex.autonomy_replay_evidence/1";

struct PrEvidenceGapInput<'a> {
	kind: String,
	source_refs: Vec<String>,
	refs_redacted: bool,
	summary: String,
	summary_redacted: bool,
	pr_head_ref: Option<&'a str>,
	pr_head_oid: Option<&'a str>,
	raw_source_refs: &'a [String],
	review_lifecycle_records: &'a [&'a ReviewLifecycleRecord],
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
	if !matching::operator_autonomy_replay_evidence_matches(payload, proposal_id, contract_ids) {
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
			match matching::operator_autonomy_matching_pr_review(
				event,
				&raw_source_refs,
				pr_head_ref,
				pr_head_oid,
				review_lifecycle_records,
			) {
				Some(review) => pr_status::operator_autonomy_pr_evidence_status_from_event(
					event,
					review,
					issue_identifier,
					summary,
					summary_redacted,
				),
				None => missing_pr_evidence_status(
					event,
					issue_identifier,
					PrEvidenceGapInput {
						kind,
						source_refs,
						refs_redacted,
						summary,
						summary_redacted,
						pr_head_ref,
						pr_head_oid,
						raw_source_refs: &raw_source_refs,
						review_lifecycle_records,
					},
				),
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

fn missing_pr_evidence_status(
	event: &PrivateExecutionEvent,
	issue_identifier: Option<&str>,
	input: PrEvidenceGapInput<'_>,
) -> OperatorAutonomyExecutionEvidenceStatus {
	let mut known_gaps = Vec::new();

	if input.source_refs.is_empty() {
		known_gaps.push(String::from("source_refs_missing_or_redacted"));
	}
	if input.refs_redacted {
		known_gaps.push(String::from("source_refs_redacted"));
	}
	if input.summary_redacted {
		known_gaps.push(String::from("summary_redacted"));
	}
	if input.pr_head_ref.is_none() || input.pr_head_oid.is_none() {
		known_gaps.push(String::from("pr_head_identity_missing"));
	} else if matching::operator_autonomy_pr_review_candidate_exists(
		event,
		input.raw_source_refs,
		input.review_lifecycle_records,
	) {
		known_gaps.push(String::from("review_lifecycle_stale_or_mismatched"));
	} else {
		known_gaps.push(String::from("review_lifecycle_missing"));
	}

	OperatorAutonomyExecutionEvidenceStatus {
		kind: input.kind,
		issue_identifier: issue_identifier.map(str::to_owned),
		source_refs: input.source_refs,
		summary: input.summary,
		updated_at: event.recorded_at().to_owned(),
		completeness: status_autonomy::operator_autonomy_completeness(&known_gaps),
		known_gaps,
	}
}
