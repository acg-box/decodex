use std::collections::{BTreeMap, BTreeSet};

use crate::agent::tracker_tool_bridge::{
	NormalizedReviewCheckpointPayload, REVIEW_POLICY_CONVERGENCE_BUDGET, ReviewFindingPolicyState,
	ReviewPolicyPhase, ReviewPolicyStatus,
	tools::review_checkpoint::{
		REVIEW_ROUTE_CURRENT_BLOCKER,
		finding_policy::{empty, record},
		routes,
	},
};

pub(in crate::agent::tracker_tool_bridge::tools) struct ReviewFindingPolicyUpdate {
	pub(in crate::agent::tracker_tool_bridge::tools) nonclean_rounds: i64,
	pub(in crate::agent::tracker_tool_bridge::tools) previous_nonclean_rounds: i64,
	pub(in crate::agent::tracker_tool_bridge::tools) finding_policy: ReviewFindingPolicyState,
}

pub(in crate::agent::tracker_tool_bridge::tools) fn review_finding_policy_update(
	previous: ReviewFindingPolicyState,
	previous_nonclean_rounds: i64,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	head_sha: &str,
	checkpoint_payload: &NormalizedReviewCheckpointPayload,
) -> ReviewFindingPolicyUpdate {
	let active_fingerprints = checkpoint_payload
		.finding_routes
		.iter()
		.filter(|route| route.route == REVIEW_ROUTE_CURRENT_BLOCKER)
		.filter_map(|route| route.finding_fingerprint.clone())
		.collect::<BTreeSet<_>>();
	let current_blocker_findings = routes::current_review_blocker_findings(checkpoint_payload)
		.map(|finding| finding.fingerprint.clone())
		.collect::<BTreeSet<_>>();
	let mut records = previous
		.findings
		.into_iter()
		.map(|review_record| (review_record.fingerprint.clone(), review_record))
		.collect::<BTreeMap<_, _>>();

	match status {
		ReviewPolicyStatus::Findings => {
			for finding in routes::current_review_blocker_findings(checkpoint_payload) {
				record::upsert_open_review_finding_record(
					&mut records,
					finding,
					head_sha,
					&checkpoint_payload.evidence,
				);
			}

			record::resolve_absent_review_findings(&mut records, &active_fingerprints);
		},
		ReviewPolicyStatus::Clean => {
			record::resolve_all_review_findings(&mut records, &checkpoint_payload.evidence);
		},
		ReviewPolicyStatus::NeedsArchitectureReview | ReviewPolicyStatus::Blocked => {},
	}

	let nonclean_rounds = if status == ReviewPolicyStatus::Findings {
		current_blocker_findings
			.iter()
			.filter_map(|fingerprint| records.get(fingerprint))
			.map(|review_record| review_record.repeat_count)
			.max()
			.unwrap_or_default()
	} else {
		0
	};
	let stop_fingerprint = current_blocker_findings
		.iter()
		.filter_map(|fingerprint| {
			records.get(fingerprint).map(|review_record| (fingerprint, review_record))
		})
		.filter(|(_fingerprint, review_record)| {
			review_record.repeat_count >= REVIEW_POLICY_CONVERGENCE_BUDGET
		})
		.max_by_key(|(_fingerprint, review_record)| review_record.repeat_count)
		.map(|(fingerprint, _review_record)| fingerprint.clone());
	let mut finding_policy =
		empty::empty_review_finding_policy(review_policy_phase, status, head_sha);

	finding_policy.nonclean_rounds = nonclean_rounds;
	finding_policy.active_fingerprints = active_fingerprints.into_iter().collect();
	finding_policy.stop_fingerprint = stop_fingerprint;
	finding_policy.findings = records.into_values().collect();

	ReviewFindingPolicyUpdate { nonclean_rounds, previous_nonclean_rounds, finding_policy }
}
