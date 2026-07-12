use crate::state::sqlite_store::{
	SqliteStateStore,
	queries::{self, RunAttemptRecord, StateData, run_attempt_record_from_row},
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

	#[cfg(test)]
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

	pub(in crate::state) fn list_run_attempts_for_lane(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> queries::Result<Vec<RunAttemptRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE project_id = ?1 AND issue_id = ?2 \
			 ORDER BY attempt_number ASC, run_id ASC",
		)?;
		let rows = statement
			.query_map(queries::params![project_id, issue_id], run_attempt_record_from_row)?;
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
