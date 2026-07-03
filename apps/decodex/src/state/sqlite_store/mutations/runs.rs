use crate::state::sqlite_store::mutations::{
	self, ChildAgentActivitySummary, IssueLease, LinearExecutionEventRuntimeRecord,
	OptionalExtension, PrivateExecutionEventRuntimeRecord, ProtocolEventRecord, Result,
	RunActivitySummaryRecord, RunAttemptRecord, RunControlChannelRecord, SqliteStateStore,
	WorktreeMappingRecord, persist, protocol_event_record_from_row,
};

impl SqliteStateStore {
	pub(in crate::state) fn upsert_run_attempt(&self, attempt: &RunAttemptRecord) -> Result<()> {
		self.connection.execute(
			"INSERT OR REPLACE INTO run_attempts (
					run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			mutations::params![
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
			mutations::params![
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
			mutations::params![
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
			mutations::params![
				lease.issue_id(),
				lease.project_id(),
				lease.run_id(),
				lease.issue_state()
			],
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
			mutations::params![
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
			mutations::params![
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
				mutations::params![run_id, sequence_number],
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
			mutations::params![
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
			mutations::params![idempotency_key],
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
			mutations::params![
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
}
