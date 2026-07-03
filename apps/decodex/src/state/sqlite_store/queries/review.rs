use crate::state::sqlite_store::{
	SqliteStateStore,
	queries::{
		self, EvidenceArtifactKey, EvidenceArtifactRuntimeRecord, LoopGuardrailKey,
		LoopGuardrailRuntimeRecord, ReviewLifecycleKey, ReviewLifecycleRuntimeRecord,
		ReviewPolicyKey, ReviewPolicyRuntimeRecord, Row, StateData,
	},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_review_lifecycle_records(
		&self,
		state: &mut StateData,
	) -> crate::state::sqlite_store::queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, branch_name, run_id, attempt_number, pr_url, \
			 target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase, \
			 request_comment_database_id, request_created_at_unix_epoch, \
			 request_description_thumbs_up_count, request_retry_count, external_round_count, \
			 auto_merge_enabled_at_unix_epoch, landing_state, closeout_state, \
			 repair_attempt_count, evidence_json, next_action, updated_at, updated_at_unix \
			 FROM review_lifecycle_records",
		)?;
		let rows = statement.query_map([], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let branch_name: String = row.get(2)?;
			let run_id: String = row.get(3)?;
			let attempt_number: i64 = row.get(4)?;
			let request_description_thumbs_up_count =
				row.get::<_, Option<i64>>(13)?.and_then(|count| usize::try_from(count).ok());

			Ok((
				ReviewLifecycleKey::new(&project_id, &issue_id, &branch_name),
				ReviewLifecycleRuntimeRecord {
					project_id,
					issue_id,
					branch_name,
					run_id,
					attempt_number,
					pr_url: row.get(5)?,
					target_base_ref_name: row.get(6)?,
					pr_head_ref_name: row.get(7)?,
					pr_head_oid: row.get(8)?,
					head_sha: row.get(9)?,
					phase: row.get(10)?,
					request_comment_database_id: row.get(11)?,
					request_created_at_unix_epoch: row.get(12)?,
					request_description_thumbs_up_count,
					request_retry_count: row.get(14)?,
					external_round_count: row.get(15)?,
					auto_merge_enabled_at_unix_epoch: row.get(16)?,
					landing_state: row.get(17)?,
					closeout_state: row.get(18)?,
					repair_attempt_count: row.get(19)?,
					evidence_json: row.get(20)?,
					next_action: row.get(21)?,
					updated_at: row.get(22)?,
					updated_at_unix: row.get(23)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.review_lifecycle_records.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_review_lifecycle_records_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> crate::state::sqlite_store::queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, branch_name, run_id, attempt_number, pr_url, \
			 target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase, \
			 request_comment_database_id, request_created_at_unix_epoch, \
			 request_description_thumbs_up_count, request_retry_count, external_round_count, \
			 auto_merge_enabled_at_unix_epoch, landing_state, closeout_state, \
			 repair_attempt_count, evidence_json, next_action, updated_at, updated_at_unix \
			 FROM review_lifecycle_records WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(queries::params![project_id], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let branch_name: String = row.get(2)?;
			let run_id: String = row.get(3)?;
			let attempt_number: i64 = row.get(4)?;
			let request_description_thumbs_up_count =
				row.get::<_, Option<i64>>(13)?.and_then(|count| usize::try_from(count).ok());

			Ok((
				ReviewLifecycleKey::new(&project_id, &issue_id, &branch_name),
				ReviewLifecycleRuntimeRecord {
					project_id,
					issue_id,
					branch_name,
					run_id,
					attempt_number,
					pr_url: row.get(5)?,
					target_base_ref_name: row.get(6)?,
					pr_head_ref_name: row.get(7)?,
					pr_head_oid: row.get(8)?,
					head_sha: row.get(9)?,
					phase: row.get(10)?,
					request_comment_database_id: row.get(11)?,
					request_created_at_unix_epoch: row.get(12)?,
					request_description_thumbs_up_count,
					request_retry_count: row.get(14)?,
					external_round_count: row.get(15)?,
					auto_merge_enabled_at_unix_epoch: row.get(16)?,
					landing_state: row.get(17)?,
					closeout_state: row.get(18)?,
					repair_attempt_count: row.get(19)?,
					evidence_json: row.get(20)?,
					next_action: row.get(21)?,
					updated_at: row.get(22)?,
					updated_at_unix: row.get(23)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.review_lifecycle_records.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_review_policy_checkpoints(
		&self,
		state: &mut StateData,
	) -> crate::state::sqlite_store::queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, run_id, attempt_number, phase, status, head_sha, \
			 nonclean_rounds, details_json, updated_at, updated_at_unix FROM review_policy_checkpoints",
		)?;
		let rows = statement.query_map([], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let run_id: String = row.get(2)?;
			let attempt_number: i64 = row.get(3)?;
			let phase: String = row.get(4)?;

			Ok((
				ReviewPolicyKey::new(&project_id, &issue_id, &run_id, attempt_number, &phase),
				ReviewPolicyRuntimeRecord {
					project_id,
					issue_id,
					run_id,
					attempt_number,
					phase,
					status: row.get(5)?,
					head_sha: row.get(6)?,
					nonclean_rounds: row.get(7)?,
					details_json: row.get(8)?,
					updated_at: row.get(9)?,
					updated_at_unix: row.get(10)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.review_policy_checkpoints.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_review_policy_checkpoints_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> crate::state::sqlite_store::queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, run_id, attempt_number, phase, status, head_sha, \
			 nonclean_rounds, details_json, updated_at, updated_at_unix FROM review_policy_checkpoints \
			 WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(queries::params![project_id], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let run_id: String = row.get(2)?;
			let attempt_number: i64 = row.get(3)?;
			let phase: String = row.get(4)?;

			Ok((
				ReviewPolicyKey::new(&project_id, &issue_id, &run_id, attempt_number, &phase),
				ReviewPolicyRuntimeRecord {
					project_id,
					issue_id,
					run_id,
					attempt_number,
					phase,
					status: row.get(5)?,
					head_sha: row.get(6)?,
					nonclean_rounds: row.get(7)?,
					details_json: row.get(8)?,
					updated_at: row.get(9)?,
					updated_at_unix: row.get(10)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.review_policy_checkpoints.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_evidence_artifacts(
		&self,
		state: &mut StateData,
	) -> crate::state::sqlite_store::queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, artifact_kind, key_hash, phase, status, head_sha, \
			 key_json, payload_json, source_run_id, source_attempt_number, updated_at, \
			 updated_at_unix FROM evidence_artifacts",
		)?;
		let rows = statement.query_map([], Self::evidence_artifact_from_row)?;

		for row in rows {
			let (key, record) = row?;

			state.evidence_artifacts.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_evidence_artifacts_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> crate::state::sqlite_store::queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, artifact_kind, key_hash, phase, status, head_sha, \
			 key_json, payload_json, source_run_id, source_attempt_number, updated_at, \
			 updated_at_unix FROM evidence_artifacts WHERE project_id = ?1",
		)?;
		let rows =
			statement.query_map(queries::params![project_id], Self::evidence_artifact_from_row)?;

		for row in rows {
			let (key, record) = row?;

			state.evidence_artifacts.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn evidence_artifact_from_row(
		row: &Row<'_>,
	) -> rusqlite::Result<(EvidenceArtifactKey, EvidenceArtifactRuntimeRecord)> {
		let project_id: String = row.get(0)?;
		let issue_id: String = row.get(1)?;
		let artifact_kind: String = row.get(2)?;
		let key_hash: String = row.get(3)?;

		Ok((
			EvidenceArtifactKey::new(&project_id, &issue_id, &artifact_kind, &key_hash),
			EvidenceArtifactRuntimeRecord {
				project_id,
				issue_id,
				artifact_kind,
				key_hash,
				phase: row.get(4)?,
				status: row.get(5)?,
				head_sha: row.get(6)?,
				key_json: row.get(7)?,
				payload_json: row.get(8)?,
				source_run_id: row.get(9)?,
				source_attempt_number: row.get(10)?,
				updated_at: row.get(11)?,
				updated_at_unix: row.get(12)?,
			},
		))
	}

	pub(in crate::state) fn load_loop_guardrail_checkpoints(
		&self,
		state: &mut StateData,
	) -> crate::state::sqlite_store::queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, reason, fingerprint, run_id, attempt_number, \
			 consecutive_count, details_json, updated_at, updated_at_unix \
			 FROM loop_guardrail_checkpoints",
		)?;
		let rows = statement.query_map([], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let reason: String = row.get(2)?;

			Ok((
				LoopGuardrailKey::new(&project_id, &issue_id, &reason),
				LoopGuardrailRuntimeRecord {
					project_id,
					issue_id,
					reason,
					fingerprint: row.get(3)?,
					run_id: row.get(4)?,
					attempt_number: row.get(5)?,
					consecutive_count: row.get(6)?,
					details_json: row.get(7)?,
					updated_at: row.get(8)?,
					updated_at_unix: row.get(9)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.loop_guardrail_checkpoints.insert(key, record);
		}

		Ok(())
	}
}
