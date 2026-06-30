use super::{
	LinearExecutionEventRecord, LinearExecutionEventRuntimeRecord,
	PrivateExecutionEventRuntimeRecord, Result, StateData, Value, params,
};

impl super::super::SqliteStateStore {
	pub(in crate::state) fn load_linear_execution_events(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT payload_json, event_unix, recorded_at, recorded_at_unix \
			 FROM linear_execution_events",
		)?;
		let rows = statement.query_map([], |row| {
			Ok((
				row.get::<_, String>(0)?,
				row.get::<_, Option<i64>>(1)?,
				row.get::<_, String>(2)?,
				row.get::<_, i64>(3)?,
			))
		})?;

		for row in rows {
			let (payload_json, event_unix, recorded_at, recorded_at_unix) = row?;
			let record = serde_json::from_str::<LinearExecutionEventRecord>(&payload_json)?;
			let record = LinearExecutionEventRuntimeRecord {
				record,
				event_unix,
				recorded_at,
				recorded_at_unix,
			};

			state.linear_execution_events.insert(record.record.idempotency_key.clone(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn list_linear_execution_events(
		&self,
		service_id: &str,
		issue_id: &str,
	) -> Result<Vec<LinearExecutionEventRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT payload_json, event_unix, recorded_at, recorded_at_unix \
			 FROM linear_execution_events \
			 WHERE service_id = ?1 AND issue_id = ?2",
		)?;
		let rows = statement.query_map(params![service_id, issue_id], |row| {
			Ok((
				row.get::<_, String>(0)?,
				row.get::<_, Option<i64>>(1)?,
				row.get::<_, String>(2)?,
				row.get::<_, i64>(3)?,
			))
		})?;
		let mut records = Vec::new();

		for row in rows {
			let (payload_json, event_unix, recorded_at, recorded_at_unix) = row?;
			let record = serde_json::from_str::<LinearExecutionEventRecord>(&payload_json)?;

			records.push(LinearExecutionEventRuntimeRecord {
				record,
				event_unix,
				recorded_at,
				recorded_at_unix,
			});
		}

		Ok(records)
	}

	pub(in crate::state) fn load_private_execution_events(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT record_id, project_id, issue_id, run_id, attempt_number, event_type, \
			 payload_json, recorded_at, recorded_at_unix \
			 FROM private_execution_events \
			 ORDER BY record_id ASC",
		)?;
		let rows = statement.query_map([], |row| {
			Ok((
				row.get::<_, i64>(0)?,
				row.get::<_, String>(1)?,
				row.get::<_, String>(2)?,
				row.get::<_, String>(3)?,
				row.get::<_, i64>(4)?,
				row.get::<_, String>(5)?,
				row.get::<_, String>(6)?,
				row.get::<_, String>(7)?,
				row.get::<_, i64>(8)?,
			))
		})?;

		for row in rows {
			let (
				record_id,
				project_id,
				issue_id,
				run_id,
				attempt_number,
				event_type,
				payload_json,
				recorded_at,
				recorded_at_unix,
			) = row?;
			let payload = serde_json::from_str::<Value>(&payload_json)?;

			state.private_execution_events.push(PrivateExecutionEventRuntimeRecord {
				record_id,
				project_id,
				issue_id,
				run_id,
				attempt_number,
				event_type,
				payload,
				recorded_at,
				recorded_at_unix,
			});
		}

		Ok(())
	}

	pub(in crate::state) fn load_private_execution_events_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT record_id, project_id, issue_id, run_id, attempt_number, event_type, \
			 payload_json, recorded_at, recorded_at_unix \
			 FROM private_execution_events \
			 WHERE project_id = ?1 \
			 ORDER BY record_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], |row| {
			Ok((
				row.get::<_, i64>(0)?,
				row.get::<_, String>(1)?,
				row.get::<_, String>(2)?,
				row.get::<_, String>(3)?,
				row.get::<_, i64>(4)?,
				row.get::<_, String>(5)?,
				row.get::<_, String>(6)?,
				row.get::<_, String>(7)?,
				row.get::<_, i64>(8)?,
			))
		})?;

		for row in rows {
			let (
				record_id,
				project_id,
				issue_id,
				run_id,
				attempt_number,
				event_type,
				payload_json,
				recorded_at,
				recorded_at_unix,
			) = row?;
			let payload = serde_json::from_str::<Value>(&payload_json)?;

			state.private_execution_events.push(PrivateExecutionEventRuntimeRecord {
				record_id,
				project_id,
				issue_id,
				run_id,
				attempt_number,
				event_type,
				payload,
				recorded_at,
				recorded_at_unix,
			});
		}

		Ok(())
	}
}
