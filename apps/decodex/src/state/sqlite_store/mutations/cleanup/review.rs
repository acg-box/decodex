use crate::state::sqlite_store::mutations::{self, Result, SqliteStateStore};

impl SqliteStateStore {
	pub(in crate::state) fn delete_worktree_and_review_lifecycle(
		&mut self,
		issue_id: &str,
	) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction
			.execute("DELETE FROM worktrees WHERE issue_id = ?1", mutations::params![issue_id])?;
		transaction.execute(
			"DELETE FROM review_lifecycle_records WHERE issue_id = ?1",
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
}
