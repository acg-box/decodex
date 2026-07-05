use serde_json::{self, Value};

use crate::agent::tracker_tool_bridge::{
	REVIEW_POLICY_CONVERGENCE_BUDGET, ReviewCheckpointFindingArgs, ReviewFindingPolicyState,
	ReviewPolicyPhase, ReviewPolicyState, ReviewPolicyStatus,
	tools::review_checkpoint::{
		finding_policy::{empty, record},
		normalize,
	},
};

pub(in crate::agent::tracker_tool_bridge::tools) fn review_finding_policy_from_previous_state(
	previous_state: &ReviewPolicyState,
	review_policy_phase: ReviewPolicyPhase,
) -> Option<ReviewFindingPolicyState> {
	if previous_state.phase != review_policy_phase {
		return None;
	}

	let details = serde_json::from_str::<Value>(&previous_state.details_json).ok()?;

	details
		.get("finding_policy")
		.cloned()
		.and_then(|value| serde_json::from_value::<ReviewFindingPolicyState>(value).ok())
		.or_else(|| migrate_legacy_review_finding_policy(previous_state, &details))
}

fn migrate_legacy_review_finding_policy(
	previous_state: &ReviewPolicyState,
	details: &Value,
) -> Option<ReviewFindingPolicyState> {
	let mut finding_policy = empty::empty_review_finding_policy(
		previous_state.phase,
		previous_state.status,
		&previous_state.head_sha,
	);

	if previous_state.status != ReviewPolicyStatus::Findings {
		return Some(finding_policy);
	}

	let findings = details.get("accepted_findings")?.as_array()?;

	for finding_value in findings {
		let finding = serde_json::from_value::<ReviewCheckpointFindingArgs>(finding_value.clone())
			.ok()
			.and_then(|finding| {
				normalize::normalize_review_checkpoint_finding(finding, previous_state.phase).ok()
			})?;
		let mut review_record =
			record::review_finding_policy_record(&finding, &previous_state.head_sha);

		review_record.repeat_count = previous_state.nonclean_rounds.max(1);

		record::append_review_finding_repair_evidence(&mut review_record, &finding.evidence);

		finding_policy.active_fingerprints.push(finding.fingerprint.clone());
		finding_policy.findings.push(review_record);
	}

	finding_policy.nonclean_rounds = previous_state.nonclean_rounds;

	finding_policy.active_fingerprints.sort();
	finding_policy.active_fingerprints.dedup();

	finding_policy.stop_fingerprint = (previous_state.nonclean_rounds
		>= REVIEW_POLICY_CONVERGENCE_BUDGET)
		.then(|| finding_policy.active_fingerprints.first().cloned())
		.flatten();

	Some(finding_policy)
}
