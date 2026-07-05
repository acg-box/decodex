use crate::state::sqlite_store::{
	SqliteStateStore,
	queries::{
		self, PathBuf, RunAttemptRecord, RunControlChannelRecord, StateData,
		run_attempt_record_from_row,
	},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_run_attempts(&self, state: &mut StateData) -> queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts",
		)?;
		let rows = statement.query_map([], |row| {
			let run_id: String = row.get(0)?;

			Ok((
				run_id.clone(),
				RunAttemptRecord {
					run_id,
					project_id: row.get(1)?,
					issue_id: row.get(2)?,
					attempt_number: row.get(3)?,
					status: row.get(4)?,
					thread_id: row.get(5)?,
					turn_id: row.get(6)?,
					updated_at: row.get(7)?,
					updated_at_unix: row.get(8)?,
				},
			))
		})?;

		for row in rows {
			let (run_id, attempt) = row?;

			state.run_attempts.insert(run_id, attempt);
		}

		Ok(())
	}

	pub(in crate::state) fn load_run_attempts_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE project_id = ?1",
		)?;
		let rows =
			statement.query_map(queries::params![project_id], run_attempt_record_from_row)?;

		for row in rows {
			let attempt = row?;

			state.run_attempts.insert(attempt.run_id.clone(), attempt);
		}

		Ok(())
	}
}

impl SqliteStateStore {
	pub(in crate::state) fn load_run_control_channels(
		&self,
		state: &mut StateData,
	) -> queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, transport, channel_path, status, \
			 published_at, published_at_unix, updated_at, updated_at_unix \
			 FROM run_control_channels",
		)?;
		let rows = statement.query_map([], |row| {
			Ok(RunControlChannelRecord {
				run_id: row.get(0)?,
				project_id: row.get(1)?,
				issue_id: row.get(2)?,
				attempt_number: row.get(3)?,
				transport: row.get(4)?,
				channel_path: PathBuf::from(row.get::<_, String>(5)?),
				status: row.get(6)?,
				published_at: row.get(7)?,
				published_at_unix: row.get(8)?,
				updated_at: row.get(9)?,
				updated_at_unix: row.get(10)?,
			})
		})?;

		for row in rows {
			let channel = row?;

			state.control_channels.insert(channel.run_id.clone(), channel);
		}

		Ok(())
	}

	pub(in crate::state) fn load_run_control_channels_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, transport, channel_path, status, \
			 published_at, published_at_unix, updated_at, updated_at_unix \
			 FROM run_control_channels WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(queries::params![project_id], |row| {
			Ok(RunControlChannelRecord {
				run_id: row.get(0)?,
				project_id: row.get(1)?,
				issue_id: row.get(2)?,
				attempt_number: row.get(3)?,
				transport: row.get(4)?,
				channel_path: PathBuf::from(row.get::<_, String>(5)?),
				status: row.get(6)?,
				published_at: row.get(7)?,
				published_at_unix: row.get(8)?,
				updated_at: row.get(9)?,
				updated_at_unix: row.get(10)?,
			})
		})?;

		for row in rows {
			let channel = row?;

			state.control_channels.insert(channel.run_id.clone(), channel);
		}

		Ok(())
	}
}

impl SqliteStateStore {
	#[cfg(test)]
	pub(in crate::state) fn run_attempt_for_issue_attempt(
		&self,
		issue_id: &str,
		attempt_number: i64,
	) -> queries::Result<Option<RunAttemptRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE issue_id = ?1 AND attempt_number = ?2 \
			 ORDER BY updated_at_unix DESC, run_id DESC \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(queries::params![issue_id, attempt_number])?;

		Ok(rows.next()?.map(run_attempt_record_from_row).transpose()?)
	}

	pub(in crate::state) fn latest_run_attempt_for_issue(
		&self,
		issue_id: &str,
	) -> queries::Result<Option<RunAttemptRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE issue_id = ?1 \
			 ORDER BY attempt_number DESC, updated_at_unix DESC, run_id DESC \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(queries::params![issue_id])?;

		Ok(rows.next()?.map(run_attempt_record_from_row).transpose()?)
	}

	pub(in crate::state) fn list_run_attempts_for_issue(
		&self,
		issue_id: &str,
	) -> queries::Result<Vec<RunAttemptRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE issue_id = ?1 \
			 ORDER BY attempt_number ASC, run_id ASC",
		)?;
		let rows = statement.query_map(queries::params![issue_id], run_attempt_record_from_row)?;

		rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
	}

	pub(in crate::state) fn list_run_attempts_for_project(
		&self,
		project_id: &str,
	) -> queries::Result<Vec<RunAttemptRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, run_id ASC",
		)?;
		let rows =
			statement.query_map(queries::params![project_id], run_attempt_record_from_row)?;

		rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
	}
}

impl SqliteStateStore {
	pub(in crate::state) fn run_has_protocol_event(
		&self,
		run_id: &str,
		event_type: &str,
	) -> queries::Result<bool> {
		let exists = self.connection.query_row(
			"SELECT EXISTS(
			 SELECT 1 FROM protocol_events
			 WHERE run_id = ?1 AND event_type = ?2
			 LIMIT 1
			 )",
			queries::params![run_id, event_type],
			|row| row.get::<_, i64>(0),
		)?;

		Ok(exists != 0)
	}
}

impl SqliteStateStore {
	pub(in crate::state) fn retry_budget_attempt_count(
		&self,
		issue_id: &str,
	) -> queries::Result<i64> {
		self.connection
			.query_row(
				"SELECT COUNT(*) FROM run_attempts \
				 WHERE issue_id = ?1 AND status IN ('failed', 'interrupted', 'terminal_guarded')",
				queries::params![issue_id],
				|row| row.get(0),
			)
			.map_err(Into::into)
	}

	pub(in crate::state) fn issue_has_retry_budget_attempt_after(
		&self,
		issue_id: &str,
		attempt_number: i64,
	) -> queries::Result<bool> {
		let count = self.connection.query_row(
			"SELECT COUNT(*) FROM run_attempts \
			 WHERE issue_id = ?1 \
			 AND attempt_number > ?2 \
			 AND status IN ('failed', 'interrupted', 'terminal_guarded') \
			 LIMIT 1",
			queries::params![issue_id, attempt_number],
			|row| row.get::<_, i64>(0),
		)?;

		Ok(count > 0)
	}
}
