use super::{
	AutonomyObjectiveRuntimeRecord, AutonomyProposalRuntimeRecord, AutonomySignalRuntimeRecord,
	ChildAgentActivitySummary, ConnectorBackoff, DecisionContractRuntimeRecord,
	ExecutionProgramRuntimeRecord, IssueLease, LinearExecutionEventRuntimeRecord,
	OptionalExtension, PrivateExecutionEventRuntimeRecord, ProjectRegistration,
	ProtocolEventRecord, Result, RunActivitySummaryRecord, RunAttemptRecord,
	RunControlChannelRecord, StateData, WorktreeMappingRecord, connector_backoff_from_row, eyre,
	params, persist, protocol_event_record_from_row,
};

impl super::SqliteStateStore {
	pub(in crate::state) fn persist_runtime_state(&mut self, state: &StateData) -> Result<()> {
		let transaction = self.connection.transaction()?;

		persist::persist_projects(&transaction, state)?;
		persist::persist_leases(&transaction, state)?;
		persist::persist_run_attempts(&transaction, state)?;
		persist::persist_run_control_channels(&transaction, state)?;
		persist::persist_protocol_events(&transaction, state)?;
		persist::persist_run_activity_summaries(&transaction, state)?;
		persist::persist_worktrees(&transaction, state)?;
		persist::persist_linear_execution_events(&transaction, state)?;
		persist::persist_private_execution_events(&transaction, state)?;
		persist::persist_decision_contracts(&transaction, state)?;
		persist::persist_autonomy_objectives(&transaction, state)?;
		persist::persist_autonomy_signals(&transaction, state)?;
		persist::persist_autonomy_proposals(&transaction, state)?;
		persist::persist_execution_programs(&transaction, state)?;
		persist::persist_program_intake_state(&transaction, state)?;
		persist::persist_review_lifecycle_records(&transaction, state)?;
		persist::persist_review_policy_checkpoints(&transaction, state)?;
		persist::persist_evidence_artifacts(&transaction, state)?;
		persist::persist_loop_guardrail_checkpoints(&transaction, state)?;
		persist::persist_connector_backoffs(&transaction, state)?;

		transaction.commit()?;

		Ok(())
	}

	pub(in crate::state) fn delete_project(&mut self, service_id: &str) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute("DELETE FROM projects WHERE service_id = ?1", params![service_id])?;
		transaction
			.execute("DELETE FROM connector_backoffs WHERE project_id = ?1", params![service_id])?;
		transaction.execute(
			"DELETE FROM run_control_channels WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction
			.execute("DELETE FROM decision_contracts WHERE project_id = ?1", params![service_id])?;
		transaction.execute(
			"DELETE FROM autonomy_objectives WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction
			.execute("DELETE FROM autonomy_signals WHERE project_id = ?1", params![service_id])?;
		transaction
			.execute("DELETE FROM autonomy_proposals WHERE project_id = ?1", params![service_id])?;
		transaction
			.execute("DELETE FROM execution_programs WHERE project_id = ?1", params![service_id])?;
		transaction.execute(
			"DELETE FROM program_intake_plans WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM program_issue_mappings WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction
			.execute("DELETE FROM evidence_artifacts WHERE project_id = ?1", params![service_id])?;
		transaction.execute(
			"DELETE FROM loop_guardrail_checkpoints WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction.commit()?;

		Ok(())
	}

	pub(in crate::state) fn upsert_project(&self, project: &ProjectRegistration) -> Result<()> {
		self.connection.execute(
			"INSERT OR REPLACE INTO projects (
					service_id, config_path, repo_root, worktree_root, workflow_path,
					tracker_api_key_env_var, github_token_env_var, enabled, config_fingerprint,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				project.service_id(),
				project.config_path().to_string_lossy().as_ref(),
				project.repo_root().to_string_lossy().as_ref(),
				project.worktree_root().to_string_lossy().as_ref(),
				project.workflow_path().to_string_lossy().as_ref(),
				project.tracker_api_key_env_var(),
				project.github_token_env_var(),
				if project.enabled() { 1_i64 } else { 0_i64 },
				project.config_fingerprint(),
				project.updated_at(),
				project.updated_at_unix(),
			],
		)?;

		Ok(())
	}

	pub(in crate::state) fn delete_connector_backoff(
		&self,
		project_id: &str,
		connector: &str,
	) -> Result<()> {
		self.connection.execute(
			"DELETE FROM connector_backoffs WHERE project_id = ?1 AND connector = ?2",
			params![project_id, connector],
		)?;

		Ok(())
	}

	pub(in crate::state) fn connector_backoff(
		&self,
		project_id: &str,
		connector: &str,
	) -> Result<Option<ConnectorBackoff>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, connector, sync_phase, quota_class, reset_unix_epoch,
			 reset_source, warning, updated_at, updated_at_unix
			 FROM connector_backoffs
			 WHERE project_id = ?1 AND connector = ?2
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![project_id, connector])?;

		Ok(rows.next()?.map(connector_backoff_from_row).transpose()?)
	}

	pub(in crate::state) fn upsert_run_attempt(&self, attempt: &RunAttemptRecord) -> Result<()> {
		self.connection.execute(
			"INSERT OR REPLACE INTO run_attempts (
					run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				&attempt.run_id,
				attempt.project_id.as_deref(),
				&attempt.issue_id,
				attempt.attempt_number,
				&attempt.status,
				attempt.thread_id.as_deref(),
				attempt.turn_id.as_deref(),
				&attempt.updated_at,
				attempt.updated_at_unix,
			],
		)?;

		Ok(())
	}

	pub(in crate::state) fn upsert_run_control_channel(
		&self,
		channel: &RunControlChannelRecord,
	) -> Result<()> {
		self.connection.execute(
			"INSERT OR REPLACE INTO run_control_channels (
					run_id, project_id, issue_id, attempt_number, transport, channel_path, status,
					published_at, published_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				&channel.run_id,
				&channel.project_id,
				&channel.issue_id,
				channel.attempt_number,
				&channel.transport,
				channel.channel_path.to_string_lossy().as_ref(),
				&channel.status,
				&channel.published_at,
				channel.published_at_unix,
				&channel.updated_at,
				channel.updated_at_unix,
			],
		)?;

		Ok(())
	}

	pub(in crate::state) fn upsert_run_activity_summary(
		&self,
		summary: &RunActivitySummaryRecord,
	) -> Result<()> {
		let child_agent_activity_json = summary
			.child_agent_activity
			.as_ref()
			.cloned()
			.map(ChildAgentActivitySummary::sealed_durable)
			.map(|summary| serde_json::to_string(&summary))
			.transpose()?;
		let protocol_activity_json =
			summary.protocol_activity.as_ref().map(serde_json::to_string).transpose()?;

		self.connection.execute(
			"INSERT OR REPLACE INTO run_activity_summaries (
					run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
			params![
				&summary.run_id,
				summary.attempt_number,
				child_agent_activity_json.as_deref(),
				protocol_activity_json.as_deref(),
				&summary.updated_at,
				summary.updated_at_unix,
			],
		)?;

		Ok(())
	}

	pub(in crate::state) fn upsert_lease_and_remember_run_project(
		&mut self,
		lease: &IssueLease,
	) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute(
			"INSERT OR REPLACE INTO leases (issue_id, project_id, run_id, issue_state)
			 VALUES (?1, ?2, ?3, ?4)",
			params![lease.issue_id(), lease.project_id(), lease.run_id(), lease.issue_state()],
		)?;

		persist::update_run_attempt_project(
			&transaction,
			lease.project_id(),
			lease.issue_id(),
			Some(lease.run_id()),
		)?;

		transaction.commit()?;

		Ok(())
	}

	pub(in crate::state) fn upsert_worktree_and_remember_run_project(
		&mut self,
		mapping: &WorktreeMappingRecord,
	) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute(
			"INSERT OR REPLACE INTO worktrees (
				issue_id, project_id, branch_name, worktree_path,
				provenance_source, created_at_unix, updated_at_unix
			 )
			 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
			params![
				&mapping.issue_id,
				&mapping.project_id,
				&mapping.branch_name,
				mapping.worktree_path.to_string_lossy().as_ref(),
				&mapping.provenance_source,
				mapping.created_at_unix,
				mapping.updated_at_unix,
			],
		)?;

		persist::update_run_attempt_project(
			&transaction,
			&mapping.project_id,
			&mapping.issue_id,
			None,
		)?;

		transaction.commit()?;

		Ok(())
	}

	pub(in crate::state) fn append_protocol_event(
		&self,
		run_id: &str,
		event: &ProtocolEventRecord,
	) -> Result<bool> {
		let changed = self.connection.execute(
			"INSERT OR IGNORE INTO protocol_events (
					run_id, sequence_number, event_type, payload_sha256, created_at, created_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
			params![
				run_id,
				event.sequence_number,
				&event.event_type,
				&event.payload_sha256,
				&event.created_at,
				event.created_at_unix,
			],
		)?;

		Ok(changed == 1)
	}

	pub(in crate::state) fn protocol_event(
		&self,
		run_id: &str,
		sequence_number: i64,
	) -> Result<Option<ProtocolEventRecord>> {
		Ok(self
			.connection
			.query_row(
				"SELECT sequence_number, event_type, payload_sha256, created_at, created_at_unix \
				 FROM protocol_events WHERE run_id = ?1 AND sequence_number = ?2",
				params![run_id, sequence_number],
				protocol_event_record_from_row,
			)
			.optional()?)
	}

	pub(in crate::state) fn insert_linear_execution_event_if_absent(
		&self,
		record: &LinearExecutionEventRuntimeRecord,
	) -> Result<bool> {
		let payload_json = serde_json::to_string(&record.record)?;
		let changed = self.connection.execute(
			"INSERT OR IGNORE INTO linear_execution_events (
					idempotency_key, service_id, issue_id, event_type, event_timestamp,
					event_unix, payload_json, recorded_at, recorded_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				&record.record.idempotency_key,
				&record.record.service_id,
				&record.record.issue_id,
				&record.record.event_type,
				&record.record.event_timestamp,
				record.event_unix,
				payload_json,
				&record.recorded_at,
				record.recorded_at_unix,
			],
		)?;

		Ok(changed == 1)
	}

	pub(in crate::state) fn delete_linear_execution_event(
		&self,
		idempotency_key: &str,
	) -> Result<()> {
		self.connection.execute(
			"DELETE FROM linear_execution_events WHERE idempotency_key = ?1",
			params![idempotency_key],
		)?;

		Ok(())
	}

	pub(in crate::state) fn insert_private_execution_event(
		&self,
		record: &PrivateExecutionEventRuntimeRecord,
	) -> Result<i64> {
		let payload_json = serde_json::to_string(&record.payload)?;

		self.connection.execute(
			"INSERT INTO private_execution_events (
					project_id, issue_id, run_id, attempt_number, event_type, payload_json,
					recorded_at, recorded_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
			params![
				&record.project_id,
				&record.issue_id,
				&record.run_id,
				record.attempt_number,
				&record.event_type,
				payload_json,
				&record.recorded_at,
				record.recorded_at_unix,
			],
		)?;

		Ok(self.connection.last_insert_rowid())
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_decision_contract(
		&self,
		record: &DecisionContractRuntimeRecord,
	) -> Result<()> {
		let payload_json = serde_json::to_string(&record.contract)?;

		self.connection.execute(
			"INSERT INTO decision_contracts (
					project_id, contract_id, source_issue_id, status, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
			 ON CONFLICT(project_id, contract_id) DO UPDATE SET
				 source_issue_id = excluded.source_issue_id,
				 status = excluded.status,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			params![
				&record.project_id,
				record.contract.contract_id(),
				record.source_issue_id.as_deref(),
				record.status.as_str(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;

		Ok(())
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_autonomy_objective(
		&self,
		record: &AutonomyObjectiveRuntimeRecord,
	) -> Result<()> {
		let payload_json = serde_json::to_string(&record.objective)?;
		let version = i64::try_from(record.objective.version())
			.map_err(|_| eyre::eyre!("Autonomy objective version exceeds SQLite integer range."))?;

		self.connection.execute(
			"INSERT INTO autonomy_objectives (
					project_id, objective_id, version, state, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
			 ON CONFLICT(project_id, objective_id, version) DO UPDATE SET
				 state = excluded.state,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			params![
				&record.project_id,
				record.objective.id(),
				version,
				record.state.as_str(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;

		Ok(())
	}

	pub(in crate::state) fn upsert_autonomy_signal(
		&self,
		record: &AutonomySignalRuntimeRecord,
	) -> Result<()> {
		let payload_json = serde_json::to_string(&record.signal)?;
		let version = i64::try_from(record.signal.objective_version()).map_err(|_| {
			eyre::eyre!("Autonomy signal objective_version exceeds SQLite integer range.")
		})?;

		self.connection.execute(
			"INSERT INTO autonomy_signals (
					project_id, signal_id, objective_id, objective_version, kind, fingerprint,
					freshness, evidence_class, confidence, privacy, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
			 ON CONFLICT(project_id, signal_id) DO UPDATE SET
				 objective_id = excluded.objective_id,
				 objective_version = excluded.objective_version,
				 kind = excluded.kind,
				 fingerprint = excluded.fingerprint,
				 freshness = excluded.freshness,
				 evidence_class = excluded.evidence_class,
				 confidence = excluded.confidence,
				 privacy = excluded.privacy,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			params![
				&record.project_id,
				record.signal.id(),
				record.signal.objective_id(),
				version,
				record.signal.kind().as_str(),
				record.signal.fingerprint(),
				record.signal.freshness().as_str(),
				record.signal.evidence_class().as_str(),
				record.signal.confidence().as_str(),
				record.signal.privacy().as_str(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;

		Ok(())
	}

	pub(in crate::state) fn upsert_autonomy_proposal(
		&self,
		record: &AutonomyProposalRuntimeRecord,
	) -> Result<()> {
		let payload_json = serde_json::to_string(&record.proposal)?;
		let version = i64::try_from(record.proposal.objective_version()).map_err(|_| {
			eyre::eyre!("Autonomy proposal objective_version exceeds SQLite integer range.")
		})?;

		self.connection.execute(
			"INSERT INTO autonomy_proposals (
					project_id, proposal_id, objective_id, objective_version, state, fingerprint,
					source_family, intended_surface, payload_json, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
			 ON CONFLICT(project_id, proposal_id) DO UPDATE SET
				 objective_id = excluded.objective_id,
				 objective_version = excluded.objective_version,
				 state = excluded.state,
				 fingerprint = excluded.fingerprint,
				 source_family = excluded.source_family,
				 intended_surface = excluded.intended_surface,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			params![
				&record.project_id,
				record.proposal.id(),
				record.proposal.objective_id(),
				version,
				record.state.as_str(),
				record.proposal.fingerprint(),
				record.proposal.source_family(),
				record.proposal.intended_surface(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;

		Ok(())
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_execution_program(
		&self,
		record: &ExecutionProgramRuntimeRecord,
	) -> Result<()> {
		let payload_json = serde_json::to_string(&record.program)?;

		self.connection.execute(
			"INSERT INTO execution_programs (
					project_id, program_id, source_contract_id, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
			 ON CONFLICT(project_id, program_id) DO UPDATE SET
				 source_contract_id = excluded.source_contract_id,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			params![
				&record.project_id,
				record.program.program_id(),
				record.source_contract_id.as_deref(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
		self.replace_program_intake_state(record)?;

		Ok(())
	}

	pub(in crate::state) fn replace_program_intake_state(
		&self,
		record: &ExecutionProgramRuntimeRecord,
	) -> Result<()> {
		self.connection.execute(
			"DELETE FROM program_intake_plans WHERE project_id = ?1 AND program_id = ?2",
			params![&record.project_id, record.program.program_id()],
		)?;
		self.connection.execute(
			"DELETE FROM program_issue_mappings WHERE project_id = ?1 AND program_id = ?2",
			params![&record.project_id, record.program.program_id()],
		)?;

		persist::insert_program_intake_state(&self.connection, record)
	}

	pub(in crate::state) fn delete_lease(&mut self, issue_id: &str) -> Result<()> {
		self.connection.execute("DELETE FROM leases WHERE issue_id = ?1", params![issue_id])?;

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
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction
			.execute("DELETE FROM leases WHERE issue_id = ?1", params![previous_issue_id])?;
		transaction.execute(
			"INSERT OR IGNORE INTO worktrees (
				issue_id, project_id, branch_name, worktree_path,
				provenance_source, created_at_unix, updated_at_unix
			 )
			 SELECT ?2, project_id, branch_name, worktree_path,
				provenance_source, created_at_unix, updated_at_unix
			 FROM worktrees WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction
			.execute("DELETE FROM worktrees WHERE issue_id = ?1", params![previous_issue_id])?;
		transaction.execute(
			"UPDATE run_attempts SET issue_id = ?2 WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"UPDATE run_control_channels SET issue_id = ?2 WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"UPDATE private_execution_events SET issue_id = ?2 WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"UPDATE decision_contracts SET source_issue_id = ?2 WHERE source_issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"UPDATE program_issue_mappings SET issue_id = ?2 WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"INSERT OR IGNORE INTO loop_guardrail_checkpoints (
					project_id, issue_id, reason, fingerprint, run_id, attempt_number,
					consecutive_count, details_json, updated_at, updated_at_unix
				)
			 SELECT project_id, ?2, reason, fingerprint, run_id, attempt_number,
					consecutive_count, details_json, updated_at, updated_at_unix
			 FROM loop_guardrail_checkpoints WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM loop_guardrail_checkpoints WHERE issue_id = ?1",
			params![previous_issue_id],
		)?;
		transaction.execute(
			"INSERT OR IGNORE INTO review_policy_checkpoints (
					project_id, issue_id, run_id, attempt_number, phase, status, head_sha,
					nonclean_rounds, details_json, updated_at, updated_at_unix
				)
			 SELECT project_id, ?2, run_id, attempt_number, phase, status, head_sha,
					nonclean_rounds, details_json, updated_at, updated_at_unix
			 FROM review_policy_checkpoints WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM review_policy_checkpoints WHERE issue_id = ?1",
			params![previous_issue_id],
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
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM evidence_artifacts WHERE issue_id = ?1",
			params![previous_issue_id],
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
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM review_lifecycle_records WHERE issue_id = ?1",
			params![previous_issue_id],
		)?;
		transaction.commit()?;

		Ok(())
	}

	pub(in crate::state) fn delete_worktree_and_review_lifecycle(
		&mut self,
		issue_id: &str,
	) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute("DELETE FROM worktrees WHERE issue_id = ?1", params![issue_id])?;
		transaction.execute(
			"DELETE FROM review_lifecycle_records WHERE issue_id = ?1",
			params![issue_id],
		)?;
		transaction.execute(
			"DELETE FROM review_policy_checkpoints WHERE issue_id = ?1",
			params![issue_id],
		)?;
		transaction
			.execute("DELETE FROM evidence_artifacts WHERE issue_id = ?1", params![issue_id])?;
		transaction.execute(
			"DELETE FROM loop_guardrail_checkpoints WHERE issue_id = ?1",
			params![issue_id],
		)?;
		transaction.commit()?;

		Ok(())
	}

	pub(in crate::state) fn delete_worktree_mapping(&mut self, issue_id: &str) -> Result<()> {
		self.connection.execute("DELETE FROM worktrees WHERE issue_id = ?1", params![issue_id])?;

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
			params![project_id, issue_id, branch_name, run_id, attempt_number],
		)?;
		transaction.execute(
			"DELETE FROM review_policy_checkpoints
			 WHERE project_id = ?1 AND issue_id = ?2 AND run_id = ?3 AND attempt_number = ?4",
			params![project_id, issue_id, run_id, attempt_number],
		)?;
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
			params![project_id, issue_id],
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
			params![project_id, issue_id, reason],
		)?;

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
			params![project_id, issue_id, run_id, attempt_number],
		)?;

		Ok(())
	}
}
