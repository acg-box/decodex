use crate::state::sqlite_store::persist::{self, Result, StateData, Transaction};

pub(in crate::state::sqlite_store) fn persist_review_lifecycle_records(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.review_lifecycle_records.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO review_lifecycle_records (
					project_id, issue_id, branch_name, run_id, attempt_number, pr_url,
					target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha,
					phase, request_comment_database_id, request_created_at_unix_epoch,
					request_description_thumbs_up_count, request_retry_count, external_round_count,
					auto_merge_enabled_at_unix_epoch, landing_state, closeout_state,
					repair_attempt_count, evidence_json, next_action, schema_version, subject_id,
					sequence, transition, previous_state, next_state, review_level,
					review_gate_state, base_branch, validated_head_sha, worktree_path, merge_commit,
					cleanup_state, authority, actor, source_evidence_refs_json, idempotency_key,
					correlation_id, causation_id, decided_at, updated_at, updated_at_unix
				) VALUES (
					?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
					?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24,
					?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36,
					?37, ?38, ?39, ?40, ?41, ?42, ?43, ?44
				)",
			persist::params![
				record.project_id,
				record.issue_id,
				record.branch_name,
				record.run_id,
				record.attempt_number,
				record.pr_url,
				record.target_base_ref_name,
				record.pr_head_ref_name,
				record.pr_head_oid,
				record.head_sha,
				record.phase,
				record.request_comment_database_id,
				record.request_created_at_unix_epoch,
				record
					.request_description_thumbs_up_count
					.and_then(|count| i64::try_from(count).ok()),
				record.request_retry_count,
				record.external_round_count,
				record.auto_merge_enabled_at_unix_epoch,
				record.landing_state,
				record.closeout_state,
				record.repair_attempt_count,
				record.evidence_json,
				record.next_action,
				record.schema_version,
				record.subject_id,
				record.sequence,
				record.transition,
				record.previous_state,
				record.next_state,
				record.review_level,
				record.review_gate_state,
				record.base_branch,
				record.validated_head_sha,
				record.worktree_path,
				record.merge_commit,
				record.cleanup_state,
				record.authority,
				record.actor,
				record.source_evidence_refs_json,
				record.idempotency_key,
				record.correlation_id,
				record.causation_id,
				record.decided_at,
				record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn persist_review_policy_checkpoints(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.review_policy_checkpoints.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO review_policy_checkpoints (
					project_id, issue_id, run_id, attempt_number, phase, status, head_sha,
					nonclean_rounds, details_json, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			persist::params![
				record.project_id,
				record.issue_id,
				record.run_id,
				record.attempt_number,
				record.phase,
				record.status,
				record.head_sha,
				record.nonclean_rounds,
				record.details_json,
				record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn persist_evidence_artifacts(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.evidence_artifacts.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO evidence_artifacts (
					project_id, issue_id, artifact_kind, key_hash, phase, status, head_sha,
					key_json, payload_json, source_run_id, source_attempt_number, updated_at,
					updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
			persist::params![
				record.project_id,
				record.issue_id,
				record.artifact_kind,
				record.key_hash,
				record.phase,
				record.status,
				record.head_sha,
				record.key_json,
				record.payload_json,
				record.source_run_id,
				record.source_attempt_number,
				record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn persist_loop_guardrail_checkpoints(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.loop_guardrail_checkpoints.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO loop_guardrail_checkpoints (
					project_id, issue_id, reason, fingerprint, run_id, attempt_number,
					consecutive_count, details_json, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
			persist::params![
				record.project_id,
				record.issue_id,
				record.reason,
				record.fingerprint,
				record.run_id,
				record.attempt_number,
				record.consecutive_count,
				record.details_json,
				record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn persist_connector_backoffs(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.connector_backoffs.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO connector_backoffs (
					project_id, connector, sync_phase, quota_class, reset_unix_epoch,
					reset_source, warning, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			persist::params![
				record.project_id,
				record.connector,
				record.sync_phase,
				record.quota_class,
				record.reset_unix_epoch,
				record.reset_source,
				record.warning,
				record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}
