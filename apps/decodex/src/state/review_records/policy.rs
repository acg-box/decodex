use serde::Serialize;
use sha2::{Digest as _, Sha256};

use crate::{
	prelude::{Result, eyre},
	state::{
		ReviewCheckpointArtifactLookup, ReviewPolicyCheckpoint, ReviewPolicyCheckpointInput,
		StateStore,
		runtime_records::{
			EvidenceArtifactKey, EvidenceArtifactRuntimeRecord, ReviewPolicyKey,
			ReviewPolicyRuntimeRecord, TimestampParts,
		},
		runtime_row_parsers,
	},
};

const REVIEW_CHECKPOINT_PROMPT_VERSION: &str = "decodex-review-checkpoint/2";

impl StateStore {
	/// Create or replace the latest review-policy checkpoint for one run phase.
	pub(crate) fn upsert_review_policy_checkpoint(
		&self,
		input: ReviewPolicyCheckpointInput<'_>,
	) -> Result<ReviewPolicyCheckpoint> {
		let now = runtime_row_parsers::timestamp_parts();
		let key = ReviewPolicyKey::new(
			input.project_id,
			input.issue_id,
			input.run_id,
			input.attempt_number,
			input.phase,
		);
		let record = ReviewPolicyRuntimeRecord {
			project_id: input.project_id.to_owned(),
			issue_id: input.issue_id.to_owned(),
			run_id: input.run_id.to_owned(),
			attempt_number: input.attempt_number,
			phase: input.phase.to_owned(),
			status: input.status.to_owned(),
			head_sha: input.head_sha.to_owned(),
			nonclean_rounds: input.nonclean_rounds,
			details_json: input.details_json.to_owned(),
			updated_at: now.text.clone(),
			updated_at_unix: now.unix,
		};
		let mut state = self.lock()?;

		state.review_policy_checkpoints.insert(key, record.clone());

		let artifact = evidence_artifact_from_review_checkpoint_input(input, &record, &now)?;
		let artifact_key = EvidenceArtifactKey::new(
			&artifact.project_id,
			&artifact.issue_id,
			&artifact.artifact_kind,
			&artifact.key_hash,
		);

		state.evidence_artifacts.insert(artifact_key, artifact);
		self.persist_runtime_state_locked(&state)?;

		Ok(record.as_public())
	}

	/// Read the latest runtime-owned review-policy checkpoint for one run phase.
	pub(crate) fn review_policy_checkpoint(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
		phase: &str,
	) -> Result<Option<ReviewPolicyCheckpoint>> {
		let state = self.lock()?;
		let key = ReviewPolicyKey::new(project_id, issue_id, run_id, attempt_number, phase);

		Ok(state.review_policy_checkpoints.get(&key).map(ReviewPolicyRuntimeRecord::as_public))
	}

	/// Return whether any bounded-review checkpoint row owns this issue.
	pub(crate) fn issue_has_review_policy_checkpoint(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<bool> {
		let state = self.lock()?;

		Ok(state
			.review_policy_checkpoints
			.values()
			.any(|record| record.project_id == project_id && record.issue_id == issue_id))
	}

	/// Read the latest review checkpoint by its canonical reusable evidence key.
	pub(crate) fn review_checkpoint_artifact(
		&self,
		lookup: ReviewCheckpointArtifactLookup<'_>,
	) -> Result<Option<ReviewPolicyCheckpoint>> {
		let state = self.lock()?;
		let key_json = review_checkpoint_evidence_key_json(
			lookup.phase,
			lookup.review_level,
			lookup.head_sha,
		)?;
		let key_hash = evidence_artifact_key_hash("issue_review_checkpoint", &key_json);
		let key = EvidenceArtifactKey::new(
			lookup.project_id,
			lookup.issue_id,
			"issue_review_checkpoint",
			&key_hash,
		);

		state
			.evidence_artifacts
			.get(&key)
			.map(EvidenceArtifactRuntimeRecord::as_review_policy_checkpoint)
			.transpose()
	}

	/// Check whether review policy has any non-clean artifact that could fence mutation.
	pub(crate) fn has_nonclean_review_checkpoint_artifact(
		&self,
		project_id: &str,
		issue_id: &str,
		phase: &str,
	) -> Result<bool> {
		let state = self.lock()?;

		Ok(state.evidence_artifacts.values().any(|record| {
			record.project_id == project_id
				&& record.issue_id == issue_id
				&& record.artifact_kind == "issue_review_checkpoint"
				&& record.phase == phase
				&& record.status != "clean"
		}))
	}

	/// Clear review-policy checkpoints for one completed run attempt.
	pub(crate) fn clear_review_policy_checkpoints_for_run_attempt(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<()> {
		let mut state = self.lock()?;

		state.review_policy_checkpoints.retain(|key, _record| {
			key.project_id != project_id
				|| key.issue_id != issue_id
				|| key.run_id != run_id
				|| key.attempt_number != attempt_number
		});

		self.delete_review_policy_checkpoints_for_run_attempt_locked(
			project_id,
			issue_id,
			run_id,
			attempt_number,
		)
	}
}

fn evidence_artifact_from_review_checkpoint_input(
	input: ReviewPolicyCheckpointInput<'_>,
	record: &ReviewPolicyRuntimeRecord,
	now: &TimestampParts,
) -> Result<EvidenceArtifactRuntimeRecord> {
	let key_json =
		review_checkpoint_evidence_key_json(input.phase, input.review_level, input.head_sha)?;
	let payload_json = serde_json::json!({
		"schema": "decodex.review_checkpoint_artifact/1",
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
	let key_hash = evidence_artifact_key_hash("issue_review_checkpoint", &key_json);

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

fn review_checkpoint_evidence_key_json(
	phase: &str,
	review_level: &str,
	head_sha: &str,
) -> Result<String> {
	#[derive(Serialize)]
	struct ReviewCheckpointEvidenceKey<'a> {
		schema: &'static str,
		artifact_kind: &'static str,
		phase: &'a str,
		head_sha: &'a str,
		review_level: &'a str,
		review_prompt_version: &'static str,
	}

	serde_json::to_string(&ReviewCheckpointEvidenceKey {
		schema: "decodex.evidence_key/1",
		artifact_kind: "issue_review_checkpoint",
		phase,
		head_sha,
		review_level,
		review_prompt_version: REVIEW_CHECKPOINT_PROMPT_VERSION,
	})
	.map_err(|error| eyre::eyre!("failed to serialize review checkpoint evidence key: {error}"))
}

fn evidence_artifact_key_hash(artifact_kind: &str, key_json: &str) -> String {
	let payload = format!("{artifact_kind}\n{key_json}");
	let digest = Sha256::digest(payload.as_bytes());
	let mut hash = String::with_capacity(64);

	for byte in digest {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	hash
}
