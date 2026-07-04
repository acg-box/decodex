use crate::state::sqlite_store::mutations::{self, Result, SqliteStateStore};

impl SqliteStateStore {
	pub(in crate::state) fn delete_loop_guardrail_checkpoints_for_issue(
		&mut self,
		project_id: &str,
		issue_id: &str,
	) -> Result<()> {
		self.connection.execute(
			"DELETE FROM loop_guardrail_checkpoints WHERE project_id = ?1 AND issue_id = ?2",
			mutations::params![project_id, issue_id],
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
			mutations::params![project_id, issue_id, reason],
		)?;

		Ok(())
	}
}
