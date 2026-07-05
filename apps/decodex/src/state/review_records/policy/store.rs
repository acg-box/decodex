use crate::{
	prelude::Result,
	state::{
		ReviewCheckpointArtifactLookup, ReviewPolicyCheckpoint, ReviewPolicyCheckpointInput,
		StateStore,
		review_records::policy::{artifact, hash},
		runtime_records::{
			EvidenceArtifactKey, EvidenceArtifactRuntimeRecord, ReviewPolicyKey,
			ReviewPolicyRuntimeRecord,
		},
		runtime_row_parsers,
	},
};

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

		let artifact =
			artifact::evidence_artifact_from_review_checkpoint_input(input, &record, &now)?;
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
		let key_json = hash::review_checkpoint_evidence_key_json(
			lookup.phase,
			lookup.review_level,
			lookup.head_sha,
		)?;
		let key_hash = hash::evidence_artifact_key_hash("issue_review_checkpoint", &key_json);
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
