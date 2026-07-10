use rusqlite::Transaction;

use crate::state::sqlite_store::mutations::{self, Result, SqliteStateStore};

impl SqliteStateStore {
	pub(in crate::state) fn delete_execution_program(
		&mut self,
		project_id: &str,
		program_id: &str,
	) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute(
			"DELETE FROM program_issue_mappings WHERE project_id = ?1 AND program_id = ?2",
			mutations::params![project_id, program_id],
		)?;
		transaction.execute(
			"DELETE FROM program_intake_plans WHERE project_id = ?1 AND program_id = ?2",
			mutations::params![project_id, program_id],
		)?;
		transaction.execute(
			"DELETE FROM execution_programs WHERE project_id = ?1 AND program_id = ?2",
			mutations::params![project_id, program_id],
		)?;
		transaction.commit()?;

		Ok(())
	}

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

		retarget_review_lifecycle_records(&transaction, previous_issue_id, canonical_issue_id)?;

		transaction.commit()?;

		Ok(())
	}

	pub(in crate::state) fn delete_loop_guardrail_checkpoints_for_issue(
		&mut self,
		project_id: &str,
		issue_id: &str,
	) -> Result<()> {
		self.connection.execute(
			"DELETE FROM loop_guardrail_checkpoints WHERE project_id = ?1 AND issue_id = ?2",
			mutations::params![project_id, issue_id],
		)?;

		Ok(())
	}

	pub(in crate::state) fn delete_loop_guardrail_checkpoint(
		&mut self,
		project_id: &str,
		issue_id: &str,
		reason: &str,
	) -> Result<()> {
		self.connection.execute(
			"DELETE FROM loop_guardrail_checkpoints \
			 WHERE project_id = ?1 AND issue_id = ?2 AND reason = ?3",
			mutations::params![project_id, issue_id, reason],
		)?;

		Ok(())
	}

	pub(in crate::state) fn delete_worktree_and_review_ephemera(
		&mut self,
		issue_id: &str,
	) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction
			.execute("DELETE FROM worktrees WHERE issue_id = ?1", mutations::params![issue_id])?;
		transaction.execute(
			"DELETE FROM review_lifecycle_records WHERE issue_id = ?1 AND sequence <= 0",
			mutations::params![issue_id],
		)?;
		transaction.execute(
			"DELETE FROM review_policy_checkpoints WHERE issue_id = ?1",
			mutations::params![issue_id],
		)?;
		transaction.execute(
			"DELETE FROM evidence_artifacts WHERE issue_id = ?1",
			mutations::params![issue_id],
		)?;
		transaction.execute(
			"DELETE FROM loop_guardrail_checkpoints WHERE issue_id = ?1",
			mutations::params![issue_id],
		)?;
		transaction.commit()?;

		Ok(())
	}

	pub(in crate::state) fn delete_review_marker_identity(
		&mut self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute(
			"DELETE FROM review_lifecycle_records
			 WHERE project_id = ?1 AND issue_id = ?2 AND branch_name = ?3
			   AND run_id = ?4 AND attempt_number = ?5",
			mutations::params![project_id, issue_id, branch_name, run_id, attempt_number],
		)?;
		transaction.execute(
			"DELETE FROM review_policy_checkpoints
			 WHERE project_id = ?1 AND issue_id = ?2 AND run_id = ?3 AND attempt_number = ?4",
			mutations::params![project_id, issue_id, run_id, attempt_number],
		)?;
		transaction.commit()?;

		Ok(())
	}

	pub(in crate::state) fn delete_review_policy_checkpoints_for_run_attempt(
		&mut self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<()> {
		self.connection.execute(
			"DELETE FROM review_policy_checkpoints
			 WHERE project_id = ?1 AND issue_id = ?2 AND run_id = ?3 AND attempt_number = ?4",
			mutations::params![project_id, issue_id, run_id, attempt_number],
		)?;

		Ok(())
	}

	pub(in crate::state) fn delete_lease(&mut self, issue_id: &str) -> Result<()> {
		self.connection
			.execute("DELETE FROM leases WHERE issue_id = ?1", mutations::params![issue_id])?;

		Ok(())
	}

	pub(in crate::state) fn delete_worktree_mapping(&mut self, issue_id: &str) -> Result<()> {
		self.connection
			.execute("DELETE FROM worktrees WHERE issue_id = ?1", mutations::params![issue_id])?;

		Ok(())
	}
}

fn retarget_review_lifecycle_records(
	transaction: &Transaction<'_>,
	previous_issue_id: &str,
	canonical_issue_id: &str,
) -> Result<()> {
	transaction.execute(
		"INSERT OR IGNORE INTO review_lifecycle_records (
				project_id, issue_id, branch_name, run_id, attempt_number, pr_url,
				target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase,
				request_comment_database_id, request_created_at_unix_epoch,
				request_description_thumbs_up_count, request_retry_count, external_round_count,
				auto_merge_enabled_at_unix_epoch, landing_state, closeout_state,
				repair_attempt_count, evidence_json, next_action, schema_version, subject_id,
				sequence, transition, previous_state, next_state, review_level,
				review_gate_state, base_branch, validated_head_sha, worktree_path, merge_commit,
				cleanup_state, authority, actor, source_evidence_refs_json, idempotency_key,
				correlation_id, causation_id, decided_at, updated_at, updated_at_unix
			)
		 SELECT project_id, ?2, branch_name, run_id, attempt_number, pr_url,
				target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase,
				request_comment_database_id, request_created_at_unix_epoch,
				request_description_thumbs_up_count, request_retry_count, external_round_count,
				auto_merge_enabled_at_unix_epoch, landing_state, closeout_state,
				repair_attempt_count, evidence_json, next_action, schema_version, subject_id,
				sequence, transition, previous_state, next_state, review_level,
				review_gate_state, base_branch, validated_head_sha, worktree_path, merge_commit,
				cleanup_state, authority, actor, source_evidence_refs_json, idempotency_key,
				correlation_id, causation_id, decided_at, updated_at, updated_at_unix
		 FROM review_lifecycle_records WHERE issue_id = ?1",
		mutations::params![previous_issue_id, canonical_issue_id],
	)?;
	transaction.execute(
		"DELETE FROM review_lifecycle_records WHERE issue_id = ?1",
		mutations::params![previous_issue_id],
	)?;

	Ok(())
}
