use crate::state::sqlite_store::mutations::{
	self, IssueLease, Result, SqliteStateStore, WorktreeMappingRecord, persist,
};

impl SqliteStateStore {
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
}
