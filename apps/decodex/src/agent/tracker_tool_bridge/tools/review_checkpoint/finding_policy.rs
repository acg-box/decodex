use std::collections::{BTreeMap, BTreeSet};

use serde_json::{self, Value};

use crate::agent::tracker_tool_bridge::{
	NormalizedReviewCheckpointFinding, NormalizedReviewCheckpointPayload,
	REVIEW_POLICY_CONVERGENCE_BUDGET, ReviewCheckpointFindingArgs, ReviewFindingPolicyRecord,
	ReviewFindingPolicyState, ReviewPolicyPhase, ReviewPolicyState, ReviewPolicyStatus,
};

use super::{
	REVIEW_ROUTE_CURRENT_BLOCKER, normalize::normalize_review_checkpoint_finding,
	routes::current_review_blocker_findings,
};

pub(in crate::agent::tracker_tool_bridge::tools) struct ReviewFindingPolicyUpdate {
	pub(in crate::agent::tracker_tool_bridge::tools) nonclean_rounds: i64,
	pub(in crate::agent::tracker_tool_bridge::tools) previous_nonclean_rounds: i64,
	pub(in crate::agent::tracker_tool_bridge::tools) finding_policy: ReviewFindingPolicyState,
}

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint) fn empty_review_finding_policy(
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	head_sha: &str,
) -> ReviewFindingPolicyState {
	ReviewFindingPolicyState {
		schema: String::from("decodex.review_finding_policy/1"),
		phase: review_policy_phase.as_str().to_owned(),
		status: status.as_str().to_owned(),
		head_sha: head_sha.to_owned(),
		nonclean_rounds: 0,
		active_fingerprints: Vec::new(),
		stop_fingerprint: None,
		findings: Vec::new(),
	}
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
	let current_blocker_findings = current_review_blocker_findings(checkpoint_payload)
		.map(|finding| finding.fingerprint.clone())
		.collect::<BTreeSet<_>>();
	let mut records = previous
		.findings
		.into_iter()
		.map(|record| (record.fingerprint.clone(), record))
		.collect::<BTreeMap<_, _>>();

	match status {
		ReviewPolicyStatus::Findings => {
			for finding in current_review_blocker_findings(checkpoint_payload) {
				upsert_open_review_finding_record(
					&mut records,
					finding,
					head_sha,
					&checkpoint_payload.evidence,
				);
			}

			resolve_absent_review_findings(&mut records, &active_fingerprints);
		},
		ReviewPolicyStatus::Clean => {
			resolve_all_review_findings(&mut records, &checkpoint_payload.evidence);
		},
		ReviewPolicyStatus::NeedsArchitectureReview | ReviewPolicyStatus::Blocked => {},
	}

	let nonclean_rounds = if status == ReviewPolicyStatus::Findings {
		current_blocker_findings
			.iter()
			.filter_map(|fingerprint| records.get(fingerprint))
			.map(|record| record.repeat_count)
			.max()
			.unwrap_or_default()
	} else {
		0
	};
	let stop_fingerprint = current_blocker_findings
		.iter()
		.filter_map(|fingerprint| records.get(fingerprint).map(|record| (fingerprint, record)))
		.filter(|(_fingerprint, record)| record.repeat_count >= REVIEW_POLICY_CONVERGENCE_BUDGET)
		.max_by_key(|(_fingerprint, record)| record.repeat_count)
		.map(|(fingerprint, _record)| fingerprint.clone());
	let mut finding_policy = empty_review_finding_policy(review_policy_phase, status, head_sha);

	finding_policy.nonclean_rounds = nonclean_rounds;
	finding_policy.active_fingerprints = active_fingerprints.into_iter().collect();
	finding_policy.stop_fingerprint = stop_fingerprint;
	finding_policy.findings = records.into_values().collect();

	ReviewFindingPolicyUpdate { nonclean_rounds, previous_nonclean_rounds, finding_policy }
}

fn upsert_open_review_finding_record(
	records: &mut BTreeMap<String, ReviewFindingPolicyRecord>,
	finding: &NormalizedReviewCheckpointFinding,
	head_sha: &str,
	checkpoint_evidence: &[String],
) {
	let existing_open =
		records.get(&finding.fingerprint).is_some_and(|record| record.status == "open");
	let mut record = records
		.remove(&finding.fingerprint)
		.unwrap_or_else(|| review_finding_policy_record(finding, head_sha));

	record.kind = finding.kind.clone();
	record.title = finding.summary.clone();
	record.body = finding.guidance.clone();
	record.file = finding.file.clone();
	record.line_range = finding.line_range.clone();

	if existing_open {
		record.repeat_count = record.repeat_count.saturating_add(1);
	} else {
		record.first_seen_head = head_sha.to_owned();
		record.repeat_count = 1;
	}

	record.last_seen_head = head_sha.to_owned();
	record.status = String::from("open");

	append_review_finding_repair_evidence(&mut record, checkpoint_evidence);
	append_review_finding_repair_evidence(&mut record, &finding.evidence);

	records.insert(finding.fingerprint.clone(), record);
}

fn review_finding_policy_record(
	finding: &NormalizedReviewCheckpointFinding,
	head_sha: &str,
) -> ReviewFindingPolicyRecord {
	ReviewFindingPolicyRecord {
		fingerprint: finding.fingerprint.clone(),
		kind: finding.kind.clone(),
		title: finding.summary.clone(),
		body: finding.guidance.clone(),
		file: finding.file.clone(),
		line_range: finding.line_range.clone(),
		first_seen_head: head_sha.to_owned(),
		last_seen_head: head_sha.to_owned(),
		status: String::from("open"),
		repeat_count: 0,
		repair_evidence: Vec::new(),
	}
}

fn append_review_finding_repair_evidence(
	record: &mut ReviewFindingPolicyRecord,
	evidence: &[String],
) {
	for item in evidence {
		if !record.repair_evidence.iter().any(|existing| existing == item) {
			record.repair_evidence.push(item.clone());
		}
	}
}

fn resolve_absent_review_findings(
	records: &mut BTreeMap<String, ReviewFindingPolicyRecord>,
	active_fingerprints: &BTreeSet<String>,
) {
	for (fingerprint, record) in records {
		if record.status == "open" && !active_fingerprints.contains(fingerprint) {
			record.status = String::from("resolved");
		}
	}
}

fn resolve_all_review_findings(
	records: &mut BTreeMap<String, ReviewFindingPolicyRecord>,
	checkpoint_evidence: &[String],
) {
	for record in records.values_mut().filter(|record| record.status == "open") {
		record.status = String::from("resolved");

		append_review_finding_repair_evidence(record, checkpoint_evidence);
	}
}

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
	let mut finding_policy = empty_review_finding_policy(
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
				normalize_review_checkpoint_finding(finding, previous_state.phase).ok()
			})?;
		let mut record = review_finding_policy_record(&finding, &previous_state.head_sha);

		record.repeat_count = previous_state.nonclean_rounds.max(1);

		append_review_finding_repair_evidence(&mut record, &finding.evidence);

		finding_policy.active_fingerprints.push(finding.fingerprint.clone());
		finding_policy.findings.push(record);
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
