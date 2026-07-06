use crate::state::sqlite_store::mutations::{
	self, LinearExecutionEventRuntimeRecord, OptionalExtension, PrivateExecutionEventRuntimeRecord,
	ProtocolEventRecord, Result, SqliteStateStore, protocol_event_record_from_row,
};

impl SqliteStateStore {
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
