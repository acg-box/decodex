use std::collections::HashMap;

use crate::state::runtime_records::{
	EvidenceArtifactKey, EvidenceArtifactRuntimeRecord, LoopGuardrailKey,
	LoopGuardrailRuntimeRecord, ReviewLifecycleKey, ReviewLifecycleRuntimeRecord, ReviewPolicyKey,
	ReviewPolicyRuntimeRecord,
};

pub(in crate::state) fn retarget_review_lifecycle_issue(
	records: &mut HashMap<ReviewLifecycleKey, ReviewLifecycleRuntimeRecord>,
	previous_issue_id: &str,
	canonical_issue_id: &str,
) {
	let previous_keys =
		records.keys().filter(|key| key.issue_id == previous_issue_id).cloned().collect::<Vec<_>>();

	for key in previous_keys {
		let Some(mut record) = records.remove(&key) else {
			continue;
		};

		record.issue_id = canonical_issue_id.to_owned();

		records
			.entry(ReviewLifecycleKey::new(&key.project_id, canonical_issue_id, &key.branch_name))
			.or_insert(record);
	}
}

pub(in crate::state) fn retarget_review_policy_issue(
	records: &mut HashMap<ReviewPolicyKey, ReviewPolicyRuntimeRecord>,
	previous_issue_id: &str,
	canonical_issue_id: &str,
) {
	let previous_keys =
		records.keys().filter(|key| key.issue_id == previous_issue_id).cloned().collect::<Vec<_>>();

	for key in previous_keys {
		let Some(mut record) = records.remove(&key) else {
			continue;
		};

		record.issue_id = canonical_issue_id.to_owned();

		records
			.entry(ReviewPolicyKey::new(
				&key.project_id,
				canonical_issue_id,
				&key.run_id,
				key.attempt_number,
				&key.phase,
			))
			.or_insert(record);
	}
}

pub(in crate::state) fn retarget_evidence_artifact_issue(
	records: &mut HashMap<EvidenceArtifactKey, EvidenceArtifactRuntimeRecord>,
	previous_issue_id: &str,
	canonical_issue_id: &str,
) {
	let previous_keys =
		records.keys().filter(|key| key.issue_id == previous_issue_id).cloned().collect::<Vec<_>>();

	for key in previous_keys {
		let Some(mut record) = records.remove(&key) else {
			continue;
		};

		record.issue_id = canonical_issue_id.to_owned();

		records
			.entry(EvidenceArtifactKey::new(
				&key.project_id,
				canonical_issue_id,
				&key.artifact_kind,
				&key.key_hash,
			))
			.or_insert(record);
	}
}

pub(in crate::state) fn retarget_loop_guardrail_issue(
	records: &mut HashMap<LoopGuardrailKey, LoopGuardrailRuntimeRecord>,
	previous_issue_id: &str,
	canonical_issue_id: &str,
) {
	let previous_keys =
		records.keys().filter(|key| key.issue_id == previous_issue_id).cloned().collect::<Vec<_>>();

	for key in previous_keys {
		let Some(mut record) = records.remove(&key) else {
			continue;
		};

		record.issue_id = canonical_issue_id.to_owned();

		records
			.entry(LoopGuardrailKey::new(&key.project_id, canonical_issue_id, &key.reason))
			.or_insert(record);
	}
}

pub(in crate::state) fn running_run_attempt_status(status: &str) -> bool {
	matches!(status, "starting" | "running")
}
