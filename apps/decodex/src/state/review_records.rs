use std::path::PathBuf;

use serde::Serialize;
use sha2::{Digest as _, Sha256};
use time::OffsetDateTime;

use crate::{
	prelude::{Result, eyre},
	state::{
		WORKTREE_PROVENANCE_RUNTIME_RECORDED, WORKTREE_PROVENANCE_RUNTIME_RECOVERED,
		runtime_records::{
			EvidenceArtifactKey, EvidenceArtifactRuntimeRecord, LoopGuardrailKey,
			LoopGuardrailRuntimeRecord, ReviewLifecycleKey, ReviewLifecycleRuntimeRecord,
			ReviewPolicyKey, ReviewPolicyRuntimeRecord, TimestampParts, WorktreeMappingRecord,
		},
		runtime_row_parsers::timestamp_parts,
	},
};

use super::{
	LoopGuardrailCheckpoint, LoopGuardrailCheckpointInput, ReviewCheckpointArtifactLookup,
	ReviewHandoffMarker, ReviewLifecycleRecord, ReviewOrchestrationMarker, ReviewPolicyCheckpoint,
	ReviewPolicyCheckpointInput, StateStore, WorktreeMapping,
};

const REVIEW_CHECKPOINT_PROMPT_VERSION: &str = "decodex-review-checkpoint/2";

impl StateStore {
	/// Create or replace the worktree mapping for one issue.
	pub fn upsert_worktree(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
		worktree_path: &str,
	) -> Result<()> {
		let mut state = self.lock_without_refresh()?;
		let now_unix = OffsetDateTime::now_utc().unix_timestamp();
		let created_at_unix = state
			.worktrees
			.get(issue_id)
			.and_then(|mapping| mapping.created_at_unix)
			.or(Some(now_unix));
		let mapping = WorktreeMappingRecord {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			branch_name: branch_name.to_owned(),
			worktree_path: PathBuf::from(worktree_path),
			provenance_source: WORKTREE_PROVENANCE_RUNTIME_RECORDED.to_owned(),
			created_at_unix,
			updated_at_unix: Some(now_unix),
		};

		state.worktrees.insert(issue_id.to_owned(), mapping.clone());
		state.remember_run_project(project_id, issue_id, None);

		self.upsert_worktree_and_remember_run_project_locked(&mapping)
	}

	/// Create or refresh a worktree mapping reconstructed from retained local state.
	pub(crate) fn upsert_recovered_worktree(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
		worktree_path: &str,
		observed_at_unix: Option<i64>,
	) -> Result<()> {
		let mut state = self.lock_without_refresh()?;
		let existing = state.worktrees.get(issue_id);
		let existing_provenance_source = existing.map(|mapping| mapping.provenance_source.as_str());
		let provenance_source = match existing_provenance_source {
			Some(WORKTREE_PROVENANCE_RUNTIME_RECORDED) => WORKTREE_PROVENANCE_RUNTIME_RECORDED,
			Some(WORKTREE_PROVENANCE_RUNTIME_RECOVERED) => WORKTREE_PROVENANCE_RUNTIME_RECOVERED,
			_ => WORKTREE_PROVENANCE_RUNTIME_RECOVERED,
		}
		.to_owned();
		let existing_created_at_unix = existing.and_then(|mapping| mapping.created_at_unix);
		let existing_updated_at_unix = existing.and_then(|mapping| mapping.updated_at_unix);
		let created_at_unix = existing_created_at_unix.or(observed_at_unix);
		let updated_at_unix = match (existing_updated_at_unix, observed_at_unix) {
			(Some(existing), Some(observed)) => Some(existing.max(observed)),
			(Some(existing), None) => Some(existing),
			(None, observed) => observed,
		};
		let mapping = WorktreeMappingRecord {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			branch_name: branch_name.to_owned(),
			worktree_path: PathBuf::from(worktree_path),
			provenance_source,
			created_at_unix,
			updated_at_unix,
		};

		state.worktrees.insert(issue_id.to_owned(), mapping.clone());
		state.remember_run_project(project_id, issue_id, None);

		self.upsert_worktree_and_remember_run_project_locked(&mapping)
	}

	/// Create or replace the retained review handoff projection for one issue lane.
	pub(crate) fn upsert_review_handoff_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		marker: &ReviewHandoffMarker,
	) -> Result<()> {
		let now = timestamp_parts();
		let key = ReviewLifecycleKey::new(project_id, issue_id, marker.branch_name());
		let mut state = self.lock()?;
		let record = state.review_lifecycle_records.entry(key).or_insert_with(|| {
			ReviewLifecycleRuntimeRecord {
				project_id: project_id.to_owned(),
				issue_id: issue_id.to_owned(),
				branch_name: marker.branch_name().to_owned(),
				run_id: marker.run_id().to_owned(),
				attempt_number: marker.attempt_number(),
				pr_url: marker.pr_url().to_owned(),
				target_base_ref_name: marker.target_base_ref_name().map(str::to_owned),
				pr_head_ref_name: marker.pr_head_ref_name().to_owned(),
				pr_head_oid: marker.pr_head_oid().to_owned(),
				head_sha: marker.pr_head_oid().to_owned(),
				phase: String::from("request_pending"),
				request_comment_database_id: None,
				request_created_at_unix_epoch: None,
				request_description_thumbs_up_count: None,
				request_retry_count: 0,
				external_round_count: 0,
				auto_merge_enabled_at_unix_epoch: None,
				landing_state: String::from("not_started"),
				closeout_state: String::from("not_started"),
				repair_attempt_count: 0,
				evidence_json: String::from("{}"),
				next_action: String::new(),
				updated_at: now.text.clone(),
				updated_at_unix: now.unix,
			}
		});
		let same_handoff_projection = record.run_id == marker.run_id()
			&& record.attempt_number == marker.attempt_number()
			&& record.pr_url == marker.pr_url()
			&& record.target_base_ref_name.as_deref() == marker.target_base_ref_name()
			&& record.pr_head_ref_name == marker.pr_head_ref_name()
			&& record.pr_head_oid == marker.pr_head_oid();

		record.run_id = marker.run_id().to_owned();
		record.attempt_number = marker.attempt_number();
		record.pr_url = marker.pr_url().to_owned();
		record.target_base_ref_name = marker.target_base_ref_name().map(str::to_owned);
		record.pr_head_ref_name = marker.pr_head_ref_name().to_owned();
		record.pr_head_oid = marker.pr_head_oid().to_owned();

		if !same_handoff_projection {
			record.head_sha = marker.pr_head_oid().to_owned();
			record.phase = String::from("request_pending");
			record.request_comment_database_id = None;
			record.request_created_at_unix_epoch = None;
			record.request_description_thumbs_up_count = None;
			record.request_retry_count = 0;
			record.external_round_count = 0;
			record.auto_merge_enabled_at_unix_epoch = None;
			record.landing_state = String::from("not_started");
			record.closeout_state = String::from("not_started");
			record.repair_attempt_count = 0;
			record.evidence_json = String::from("{}");

			record.next_action.clear();
		}

		record.updated_at = now.text;
		record.updated_at_unix = now.unix;

		self.persist_runtime_state_locked(&state)
	}

	/// Read the retained review handoff projection for one issue branch.
	pub(crate) fn review_handoff_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
	) -> Result<Option<ReviewHandoffMarker>> {
		Ok(self.review_lifecycle_record(project_id, issue_id, branch_name)?.map(|record| {
			ReviewHandoffMarker {
				run_id: record.run_id().to_owned(),
				attempt_number: record.attempt_number(),
				branch_name: record.branch_name().to_owned(),
				pr_url: record.pr_url().to_owned(),
				target_base_ref_name: record.target_base_ref_name().map(str::to_owned),
				pr_head_ref_name: record.pr_head_ref_name().to_owned(),
				pr_head_oid: record.pr_head_oid().to_owned(),
			}
		}))
	}

	/// Read the runtime-owned review lifecycle record for one retained issue branch.
	pub(crate) fn review_lifecycle_record(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
	) -> Result<Option<ReviewLifecycleRecord>> {
		let state = self.lock()?;
		let key = ReviewLifecycleKey::new(project_id, issue_id, branch_name);

		Ok(state.review_lifecycle_records.get(&key).map(ReviewLifecycleRuntimeRecord::as_public))
	}

	/// Return whether any retained review lifecycle row owns this issue.
	pub(crate) fn issue_has_review_lifecycle_record(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<bool> {
		let state = self.lock()?;

		Ok(state
			.review_lifecycle_records
			.values()
			.any(|record| record.project_id == project_id && record.issue_id == issue_id))
	}

	/// Create or replace the retained review orchestration projection for one issue lane.
	pub(crate) fn upsert_review_orchestration_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		marker: &ReviewOrchestrationMarker,
	) -> Result<()> {
		let now = timestamp_parts();
		let key = ReviewLifecycleKey::new(project_id, issue_id, marker.branch_name());
		let mut state = self.lock()?;
		let record = state.review_lifecycle_records.entry(key).or_insert_with(|| {
			ReviewLifecycleRuntimeRecord {
				project_id: project_id.to_owned(),
				issue_id: issue_id.to_owned(),
				branch_name: marker.branch_name().to_owned(),
				run_id: marker.run_id().to_owned(),
				attempt_number: marker.attempt_number(),
				pr_url: marker.pr_url().to_owned(),
				target_base_ref_name: None,
				pr_head_ref_name: marker.branch_name().to_owned(),
				pr_head_oid: marker.head_sha().to_owned(),
				head_sha: marker.head_sha().to_owned(),
				phase: marker.phase().to_owned(),
				request_comment_database_id: None,
				request_created_at_unix_epoch: None,
				request_description_thumbs_up_count: None,
				request_retry_count: 0,
				external_round_count: 0,
				auto_merge_enabled_at_unix_epoch: None,
				landing_state: String::from("not_started"),
				closeout_state: String::from("not_started"),
				repair_attempt_count: 0,
				evidence_json: String::from("{}"),
				next_action: String::new(),
				updated_at: now.text.clone(),
				updated_at_unix: now.unix,
			}
		});

		record.run_id = marker.run_id().to_owned();
		record.attempt_number = marker.attempt_number();
		record.pr_url = marker.pr_url().to_owned();
		record.head_sha = marker.head_sha().to_owned();
		record.phase = marker.phase().to_owned();
		record.request_comment_database_id = marker.request_comment_database_id();
		record.request_created_at_unix_epoch = marker.request_created_at_unix_epoch();
		record.request_description_thumbs_up_count = marker.request_description_thumbs_up_count();
		record.request_retry_count = marker.request_retry_count();
		record.external_round_count = marker.external_round_count();
		record.auto_merge_enabled_at_unix_epoch = marker.auto_merge_enabled_at_unix_epoch();
		record.updated_at = now.text;
		record.updated_at_unix = now.unix;

		self.persist_runtime_state_locked(&state)
	}

	/// Read retained review orchestration for the current handoff identity.
	pub(crate) fn review_orchestration_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		review_handoff: &ReviewHandoffMarker,
	) -> Result<Option<ReviewOrchestrationMarker>> {
		let Some(record) =
			self.review_lifecycle_record(project_id, issue_id, review_handoff.branch_name())?
		else {
			return Ok(None);
		};

		if record.run_id() != review_handoff.run_id()
			|| record.attempt_number() != review_handoff.attempt_number()
			|| record.branch_name() != review_handoff.branch_name()
			|| record.pr_url() != review_handoff.pr_url()
		{
			return Ok(None);
		}

		Ok(Some(ReviewOrchestrationMarker::new(
			record.run_id().to_owned(),
			record.attempt_number(),
			record.branch_name().to_owned(),
			record.pr_url().to_owned(),
			record.head_sha().to_owned(),
			record.phase().to_owned(),
			record.request_comment_database_id(),
			record.request_created_at_unix_epoch(),
			record.request_description_thumbs_up_count(),
			record.request_retry_count(),
			record.external_round_count(),
			record.auto_merge_enabled_at_unix_epoch(),
		)))
	}

	/// Create or replace the latest review-policy checkpoint for one run phase.
	pub(crate) fn upsert_review_policy_checkpoint(
		&self,
		input: ReviewPolicyCheckpointInput<'_>,
	) -> Result<ReviewPolicyCheckpoint> {
		let now = timestamp_parts();
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

	/// Record one loop-guardrail observation and return its consecutive count.
	pub(crate) fn observe_loop_guardrail_checkpoint(
		&self,
		input: LoopGuardrailCheckpointInput<'_>,
	) -> Result<LoopGuardrailCheckpoint> {
		let now = timestamp_parts();
		let key = LoopGuardrailKey::new(input.project_id, input.issue_id, input.reason);
		let mut state = self.lock()?;
		let previous = state.loop_guardrail_checkpoints.get(&key);
		let consecutive_count = previous.map_or(1, |record| {
			if record.fingerprint == input.fingerprint {
				record.consecutive_count.saturating_add(1)
			} else {
				1
			}
		});
		let record = LoopGuardrailRuntimeRecord {
			project_id: input.project_id.to_owned(),
			issue_id: input.issue_id.to_owned(),
			reason: input.reason.to_owned(),
			fingerprint: input.fingerprint.to_owned(),
			run_id: input.run_id.to_owned(),
			attempt_number: input.attempt_number,
			consecutive_count,
			details_json: input.details_json.to_owned(),
			updated_at: now.text,
			updated_at_unix: now.unix,
		};

		state.loop_guardrail_checkpoints.insert(key, record.clone());
		self.persist_runtime_state_locked(&state)?;

		Ok(record.as_public())
	}

	/// Read one loop-guardrail checkpoint by project, issue, and reason.
	#[cfg(test)]
	pub(crate) fn loop_guardrail_checkpoint(
		&self,
		project_id: &str,
		issue_id: &str,
		reason: &str,
	) -> Result<Option<LoopGuardrailCheckpoint>> {
		let state = self.lock()?;
		let key = LoopGuardrailKey::new(project_id, issue_id, reason);

		Ok(state.loop_guardrail_checkpoints.get(&key).map(LoopGuardrailRuntimeRecord::as_public))
	}

	/// Clear loop-guardrail checkpoints for one issue.
	pub(crate) fn clear_loop_guardrail_checkpoints_for_issue(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<()> {
		let mut state = self.lock()?;

		state
			.loop_guardrail_checkpoints
			.retain(|key, _record| key.project_id != project_id || key.issue_id != issue_id);

		self.delete_loop_guardrail_checkpoints_for_issue_locked(project_id, issue_id)
	}

	/// Clear one loop-guardrail checkpoint reason for one issue.
	pub(crate) fn clear_loop_guardrail_checkpoint(
		&self,
		project_id: &str,
		issue_id: &str,
		reason: &str,
	) -> Result<()> {
		let key = LoopGuardrailKey::new(project_id, issue_id, reason);
		let mut state = self.lock()?;

		state.loop_guardrail_checkpoints.remove(&key);

		self.delete_loop_guardrail_checkpoint_locked(project_id, issue_id, reason)
	}

	/// Remove the exact review lifecycle record created for one handoff identity.
	pub(crate) fn clear_review_lifecycle_for_handoff(
		&self,
		project_id: &str,
		issue_id: &str,
		handoff_marker: &ReviewHandoffMarker,
		orchestration_marker: &ReviewOrchestrationMarker,
	) -> Result<()> {
		let lifecycle_key =
			ReviewLifecycleKey::new(project_id, issue_id, handoff_marker.branch_name());
		let mut state = self.lock()?;

		if state
			.review_lifecycle_records
			.get(&lifecycle_key)
			.is_some_and(|record| record.matches_handoff_identity(handoff_marker))
		{
			state.review_lifecycle_records.remove(&lifecycle_key);
		}

		state.review_policy_checkpoints.retain(|key, _record| {
			key.project_id != project_id
				|| key.issue_id != issue_id
				|| key.run_id != orchestration_marker.run_id()
				|| key.attempt_number != orchestration_marker.attempt_number()
		});
		self.persist_runtime_state_locked(&state)?;

		self.delete_review_marker_identity_locked(
			project_id,
			issue_id,
			handoff_marker.branch_name(),
			orchestration_marker.run_id(),
			orchestration_marker.attempt_number(),
		)
	}

	/// Read the worktree mapping for one issue.
	pub fn worktree_for_issue(&self, issue_id: &str) -> Result<Option<WorktreeMapping>> {
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.worktree_for_issue(issue_id)
				.map(|mapping| mapping.map(|mapping| mapping.as_public()));
		}

		let state = self.lock()?;

		Ok(state.worktrees.get(issue_id).map(WorktreeMappingRecord::as_public))
	}

	/// List all known worktree mappings.
	pub fn list_worktrees(&self, project_id: &str) -> Result<Vec<WorktreeMapping>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;

		let mut mappings = state
			.worktrees
			.values()
			.filter(|mapping| mapping.project_id == project_id)
			.map(WorktreeMappingRecord::as_public)
			.collect::<Vec<_>>();

		mappings.sort_by(|left, right| left.issue_id.cmp(&right.issue_id));

		Ok(mappings)
	}

	/// Remove the worktree mapping for one issue.
	pub fn clear_worktree(&self, issue_id: &str) -> Result<()> {
		let mut state = self.lock()?;

		state.worktrees.remove(issue_id);
		state.review_lifecycle_records.retain(|key, _record| key.issue_id != issue_id);
		state.review_policy_checkpoints.retain(|key, _record| key.issue_id != issue_id);
		self.persist_runtime_state_locked(&state)?;

		self.delete_worktree_and_review_lifecycle_locked(issue_id)
	}

	/// Remove only the worktree mapping for one issue.
	pub(crate) fn clear_worktree_mapping(&self, issue_id: &str) -> Result<()> {
		let mut state = self.lock()?;

		state.worktrees.remove(issue_id);
		self.persist_runtime_state_locked(&state)?;

		self.delete_worktree_mapping_locked(issue_id)
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
