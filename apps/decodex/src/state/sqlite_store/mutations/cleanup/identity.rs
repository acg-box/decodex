use crate::state::sqlite_store::mutations::{self, Result, SqliteStateStore};

impl SqliteStateStore {
	pub(in crate::state) fn retarget_issue_identity(
		&mut self,
		previous_issue_id: &str,
		canonical_issue_id: &str,
	) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute(
			"INSERT OR IGNORE INTO leases (issue_id, project_id, run_id, issue_state)
			 SELECT ?2, project_id, run_id, issue_state FROM leases WHERE issue_id = ?1",
			mutations::params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM leases WHERE issue_id = ?1",
			mutations::params![previous_issue_id],
		)?;
		transaction.execute(
			"INSERT OR IGNORE INTO worktrees (
				issue_id, project_id, branch_name, worktree_path,
				provenance_source, created_at_unix, updated_at_unix
			 )
			 SELECT ?2, project_id, branch_name, worktree_path,
				provenance_source, created_at_unix, updated_at_unix
			 FROM worktrees WHERE issue_id = ?1",
			mutations::params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM worktrees WHERE issue_id = ?1",
			mutations::params![previous_issue_id],
		)?;
		transaction.execute(
			"UPDATE run_attempts SET issue_id = ?2 WHERE issue_id = ?1",
			mutations::params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"UPDATE run_control_channels SET issue_id = ?2 WHERE issue_id = ?1",
			mutations::params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"UPDATE private_execution_events SET issue_id = ?2 WHERE issue_id = ?1",
			mutations::params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"UPDATE decision_contracts SET source_issue_id = ?2 WHERE source_issue_id = ?1",
			mutations::params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"UPDATE program_issue_mappings SET issue_id = ?2 WHERE issue_id = ?1",
			mutations::params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"INSERT OR IGNORE INTO loop_guardrail_checkpoints (
					project_id, issue_id, reason, fingerprint, run_id, attempt_number,
					consecutive_count, details_json, updated_at, updated_at_unix
				)
			 SELECT project_id, ?2, reason, fingerprint, run_id, attempt_number,
					consecutive_count, details_json, updated_at, updated_at_unix
			 FROM loop_guardrail_checkpoints WHERE issue_id = ?1",
			mutations::params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM loop_guardrail_checkpoints WHERE issue_id = ?1",
			mutations::params![previous_issue_id],
		)?;
		transaction.execute(
			"INSERT OR IGNORE INTO review_policy_checkpoints (
					project_id, issue_id, run_id, attempt_number, phase, status, head_sha,
					nonclean_rounds, details_json, updated_at, updated_at_unix
				)
			 SELECT project_id, ?2, run_id, attempt_number, phase, status, head_sha,
					nonclean_rounds, details_json, updated_at, updated_at_unix
			 FROM review_policy_checkpoints WHERE issue_id = ?1",
			mutations::params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM review_policy_checkpoints WHERE issue_id = ?1",
			mutations::params![previous_issue_id],
		)?;
		transaction.execute(
			"INSERT OR IGNORE INTO evidence_artifacts (
					project_id, issue_id, artifact_kind, key_hash, phase, status, head_sha,
					key_json, payload_json, source_run_id, source_attempt_number, updated_at,
					updated_at_unix
				)
			 SELECT project_id, ?2, artifact_kind, key_hash, phase, status, head_sha,
					key_json, payload_json, source_run_id, source_attempt_number, updated_at,
					updated_at_unix
			 FROM evidence_artifacts WHERE issue_id = ?1",
			mutations::params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM evidence_artifacts WHERE issue_id = ?1",
			mutations::params![previous_issue_id],
		)?;
		transaction.execute(
			"INSERT OR IGNORE INTO review_lifecycle_records (
					project_id, issue_id, branch_name, run_id, attempt_number, pr_url,
					target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase,
					request_comment_database_id, request_created_at_unix_epoch,
					request_description_thumbs_up_count, request_retry_count, external_round_count,
					auto_merge_enabled_at_unix_epoch, landing_state, closeout_state,
					repair_attempt_count, evidence_json, next_action, updated_at, updated_at_unix
				)
			 SELECT project_id, ?2, branch_name, run_id, attempt_number, pr_url,
					target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase,
					request_comment_database_id, request_created_at_unix_epoch,
					request_description_thumbs_up_count, request_retry_count, external_round_count,
					auto_merge_enabled_at_unix_epoch, landing_state, closeout_state,
					repair_attempt_count, evidence_json, next_action, updated_at, updated_at_unix
			 FROM review_lifecycle_records WHERE issue_id = ?1",
			mutations::params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM review_lifecycle_records WHERE issue_id = ?1",
			mutations::params![previous_issue_id],
		)?;
		transaction.commit()?;

		Ok(())
	}
}
