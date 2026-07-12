use crate::state::sqlite_store::{
	SqliteStateStore,
	queries::{self, Result},
};

impl SqliteStateStore {
	pub(in crate::state) fn retry_budget_attempt_count_for_lane(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<i64> {
		self.connection
			.query_row(
				"SELECT COUNT(*) FROM run_attempts \
				 WHERE project_id = ?1 AND issue_id = ?2 \
				 AND status IN ('failed', 'interrupted', 'terminal_guarded')",
				queries::params![project_id, issue_id],
				|row| row.get(0),
			)
			.map_err(Into::into)
	}

	pub(in crate::state) fn lane_has_retry_budget_attempt_after(
		&self,
		project_id: &str,
		issue_id: &str,
		attempt_number: i64,
	) -> Result<bool> {
		let count = self.connection.query_row(
			"SELECT COUNT(*) FROM run_attempts \
			 WHERE project_id = ?1 AND issue_id = ?2 AND attempt_number > ?3 \
			 AND status IN ('failed', 'interrupted', 'terminal_guarded')",
			queries::params![project_id, issue_id, attempt_number],
			|row| row.get::<_, i64>(0),
		)?;
		Ok(count > 0)
	}

	#[cfg(test)]
	pub(in crate::state) fn retry_budget_attempt_count(&self, issue_id: &str) -> Result<i64> {
		self.connection
			.query_row(
				"SELECT COUNT(*) FROM run_attempts \
				 WHERE issue_id = ?1 AND status IN ('failed', 'interrupted', 'terminal_guarded')",
				queries::params![issue_id],
				|row| row.get(0),
			)
			.map_err(Into::into)
	}

	#[cfg(test)]
	pub(in crate::state) fn issue_has_retry_budget_attempt_after(
		&self,
		issue_id: &str,
		attempt_number: i64,
	) -> Result<bool> {
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
