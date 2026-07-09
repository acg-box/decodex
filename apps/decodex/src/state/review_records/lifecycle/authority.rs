#![allow(dead_code)]

use crate::{
	orchestrator::kernel::lifecycle::{
		LIFECYCLE_EVENT_TYPE, LifecycleAuthorityRecord, LifecycleDecision, LifecycleEventEnvelope,
	},
	prelude::Result,
	state::{
		PrivateExecutionEvent, StateStore,
		runtime_records::{
			PrivateExecutionEventRuntimeRecord, ReviewLifecycleKey, ReviewLifecycleRuntimeRecord,
		},
		runtime_row_parsers,
	},
};

impl StateStore {
	/// Runtime state adapter for canonical lifecycle authority.
	///
	/// This is the only writer that may persist a final lifecycle state: it writes the
	/// authority projection and the append-only lifecycle event in one runtime-state
	/// transaction.
	pub(crate) fn record_lifecycle_decision(
		&self,
		run_id: &str,
		attempt_number: i64,
		decision: &LifecycleDecision,
	) -> Result<PrivateExecutionEvent> {
		let payload = serde_json::to_value(&decision.authority_record_envelope)?;
		let now = runtime_row_parsers::timestamp_parts();
		let record = &decision.authority_record;
		let lifecycle_key =
			ReviewLifecycleKey::new(&record.project_id, &record.issue_id, &record.head_branch);
		let mut state = self.lock_without_refresh()?;
		let mut event = PrivateExecutionEventRuntimeRecord {
			record_id: state.next_private_execution_event_id()?,
			project_id: record.project_id.clone(),
			issue_id: record.issue_id.clone(),
			run_id: run_id.to_owned(),
			attempt_number,
			event_type: LIFECYCLE_EVENT_TYPE.to_owned(),
			payload,
			recorded_at: now.text.clone(),
			recorded_at_unix: now.unix,
		};

		if let Some(existing) = state.private_execution_events.iter().find(|event| {
			event.project_id == record.project_id
				&& event.issue_id == record.issue_id
				&& event.event_type == LIFECYCLE_EVENT_TYPE
				&& event.payload.get("idempotency_key").and_then(|value| value.as_str())
					== Some(record.idempotency_key.as_str())
		}) {
			return Ok(existing.as_public());
		}

		upsert_lifecycle_authority_projection(
			state.review_lifecycle_records.entry(lifecycle_key).or_insert_with(|| {
				new_lifecycle_authority_record(run_id, attempt_number, record, &now.text, now.unix)
			}),
			record,
			&decision.authority_record_envelope,
			&now.text,
			now.unix,
		)?;

		state.private_execution_events.push(event.clone());
		self.persist_runtime_state_locked(&state)?;

		event.record_id = state.next_private_execution_event_id()?.saturating_sub(1);

		Ok(event.as_public())
	}
}

fn new_lifecycle_authority_record(
	run_id: &str,
	attempt_number: i64,
	record: &LifecycleAuthorityRecord,
	updated_at: &str,
	updated_at_unix: i64,
) -> ReviewLifecycleRuntimeRecord {
	ReviewLifecycleRuntimeRecord {
		project_id: record.project_id.clone(),
		issue_id: record.issue_id.clone(),
		branch_name: record.head_branch.clone(),
		run_id: run_id.to_owned(),
		attempt_number,
		pr_url: record.pr_url.clone(),
		target_base_ref_name: record.base_branch.clone(),
		pr_head_ref_name: record.head_branch.clone(),
		pr_head_oid: record.validated_head_sha.clone(),
		head_sha: record.validated_head_sha.clone(),
		phase: record.phase.clone(),
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
		next_action: record.next_action.clone(),
		schema_version: record.schema_version.clone(),
		subject_id: record.subject_id.clone(),
		sequence: record.sequence,
		transition: record.transition.clone(),
		previous_state: record.previous_state.clone(),
		next_state: record.next_state.clone(),
		review_level: record.review_level.clone(),
		review_gate_state: record.review_gate_state.clone(),
		base_branch: record.base_branch.clone(),
		validated_head_sha: record.validated_head_sha.clone(),
		worktree_path: record.worktree_path.clone(),
		merge_commit: record.merge_commit.clone(),
		cleanup_state: record.cleanup_state.clone(),
		authority: record.authority.clone(),
		actor: record.actor.clone(),
		source_evidence_refs_json: String::from("[]"),
		idempotency_key: record.idempotency_key.clone(),
		correlation_id: record.correlation_id.clone(),
		causation_id: record.causation_id.clone(),
		decided_at: record.decided_at.clone(),
		updated_at: updated_at.to_owned(),
		updated_at_unix,
	}
}

fn upsert_lifecycle_authority_projection(
	runtime_record: &mut ReviewLifecycleRuntimeRecord,
	record: &LifecycleAuthorityRecord,
	envelope: &LifecycleEventEnvelope,
	updated_at: &str,
	updated_at_unix: i64,
) -> Result<()> {
	runtime_record.pr_url = record.pr_url.clone();
	runtime_record.target_base_ref_name = record.base_branch.clone();
	runtime_record.pr_head_ref_name = record.head_branch.clone();
	runtime_record.pr_head_oid = record.validated_head_sha.clone();
	runtime_record.head_sha = record.validated_head_sha.clone();
	runtime_record.phase = record.phase.clone();

	let landing_state = landing_state_from_record(record);

	if landing_state != "not_started" || runtime_record.landing_state == "not_started" {
		runtime_record.landing_state = landing_state;
	}

	let closeout_state = closeout_state_from_record(record);

	if closeout_state != "not_started" || runtime_record.closeout_state == "not_started" {
		runtime_record.closeout_state = closeout_state;
	}

	runtime_record.evidence_json = serde_json::to_string(envelope)?;
	runtime_record.next_action = record.next_action.clone();
	runtime_record.schema_version = record.schema_version.clone();
	runtime_record.subject_id = record.subject_id.clone();
	runtime_record.sequence = record.sequence;
	runtime_record.transition = record.transition.clone();
	runtime_record.previous_state = record.previous_state.clone();
	runtime_record.next_state = record.next_state.clone();
	runtime_record.review_level = record.review_level.clone();
	runtime_record.review_gate_state = record.review_gate_state.clone();
	runtime_record.base_branch = record.base_branch.clone();
	runtime_record.validated_head_sha = record.validated_head_sha.clone();
	runtime_record.worktree_path = record.worktree_path.clone();
	runtime_record.merge_commit = record.merge_commit.clone();
	runtime_record.cleanup_state = record.cleanup_state.clone();
	runtime_record.authority = record.authority.clone();
	runtime_record.actor = record.actor.clone();
	runtime_record.source_evidence_refs_json = serde_json::to_string(&record.source_evidence_refs)?;
	runtime_record.idempotency_key = record.idempotency_key.clone();
	runtime_record.correlation_id = record.correlation_id.clone();
	runtime_record.causation_id = record.causation_id.clone();
	runtime_record.decided_at = record.decided_at.clone();
	runtime_record.updated_at = updated_at.to_owned();
	runtime_record.updated_at_unix = updated_at_unix;

	Ok(())
}

fn landing_state_from_record(record: &LifecycleAuthorityRecord) -> String {
	match record.next_state.as_str() {
		"landing_started" => "started",
		"landed" => "landed",
		"landing_failed" => "failed",
		"manual_attention_required" => "manual_attention_required",
		_ => "not_started",
	}
	.to_owned()
}

fn closeout_state_from_record(record: &LifecycleAuthorityRecord) -> String {
	match record.next_state.as_str() {
		"closed" => "completed",
		"closeout_failed" => "failed",
		"manual_attention_required" => "manual_attention_required",
		_ => "not_started",
	}
	.to_owned()
}
