use std::collections::BTreeSet;

use serde_json::Value;

use crate::state::{PrivateExecutionEvent, ReviewLifecycleRecord};

pub(in crate::orchestrator::status_autonomy::evidence::replay) fn operator_autonomy_matching_pr_review<
	'a,
>(
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

pub(in crate::orchestrator::status_autonomy::evidence::replay) fn operator_autonomy_pr_review_candidate_exists(
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

pub(in crate::orchestrator::status_autonomy::evidence::replay) fn operator_autonomy_replay_evidence_matches(
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
