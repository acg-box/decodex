use std::collections::{BTreeMap, BTreeSet};

use crate::agent::tracker_tool_bridge::{
	NormalizedReviewCheckpointFinding, ReviewFindingPolicyRecord,
};

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint::finding_policy) fn upsert_open_review_finding_record(
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

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint::finding_policy) fn review_finding_policy_record(
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

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint::finding_policy) fn append_review_finding_repair_evidence(
	record: &mut ReviewFindingPolicyRecord,
	evidence: &[String],
) {
	for item in evidence {
		if !record.repair_evidence.iter().any(|existing| existing == item) {
			record.repair_evidence.push(item.clone());
		}
	}
}

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint::finding_policy) fn resolve_absent_review_findings(
	records: &mut BTreeMap<String, ReviewFindingPolicyRecord>,
	active_fingerprints: &BTreeSet<String>,
) {
	for (fingerprint, record) in records {
		if record.status == "open" && !active_fingerprints.contains(fingerprint) {
			record.status = String::from("resolved");
		}
	}
}

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint::finding_policy) fn resolve_all_review_findings(
	records: &mut BTreeMap<String, ReviewFindingPolicyRecord>,
	checkpoint_evidence: &[String],
) {
	for record in records.values_mut().filter(|record| record.status == "open") {
		record.status = String::from("resolved");

		append_review_finding_repair_evidence(record, checkpoint_evidence);
	}
}
