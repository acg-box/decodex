use crate::state::sqlite_store::{
	SqliteStateStore,
	queries::{self, Result, RunAttemptRecord, StateData, run_attempt_record_from_row},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_run_attempts(&self, state: &mut StateData) -> Result<()> {
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
	) -> Result<()> {
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
