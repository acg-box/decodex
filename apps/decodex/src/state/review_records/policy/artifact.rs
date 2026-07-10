use crate::{
	prelude::Result,
	state::{
		ReviewPolicyCheckpointInput,
		review_records::policy::hash,
		runtime_records::{
			EvidenceArtifactRuntimeRecord, ReviewPolicyRuntimeRecord, TimestampParts,
		},
	},
};

pub(in crate::state::review_records::policy) fn evidence_artifact_from_review_checkpoint_input(
	input: ReviewPolicyCheckpointInput<'_>,
	record: &ReviewPolicyRuntimeRecord,
	now: &TimestampParts,
) -> Result<EvidenceArtifactRuntimeRecord> {
	let key_json =
		hash::review_checkpoint_evidence_key_json(input.phase, input.review_level, input.head_sha)?;
	let payload_json = serde_json::json!({
		"schema": "decodex.review_checkpoint_artifact/2",
		"phase": input.phase,
		"review_level": input.review_level,
		"status": input.status,
		"head_sha": input.head_sha,
		"nonclean_rounds": input.nonclean_rounds,
		"details_json": input.details_json,
		"source": {
			"run_id": input.run_id,
			"attempt_number": input.attempt_number
		}
	});
	let key_hash = hash::evidence_artifact_key_hash("issue_review_checkpoint", &key_json);

	Ok(EvidenceArtifactRuntimeRecord {
		project_id: record.project_id.clone(),
		issue_id: record.issue_id.clone(),
		artifact_kind: String::from("issue_review_checkpoint"),
		key_hash,
		phase: record.phase.clone(),
		status: record.status.clone(),
		head_sha: Some(record.head_sha.clone()),
		key_json,
		payload_json: payload_json.to_string(),
		source_run_id: record.run_id.clone(),
		source_attempt_number: record.attempt_number,
		updated_at: now.text.clone(),
		updated_at_unix: now.unix,
	})
}
