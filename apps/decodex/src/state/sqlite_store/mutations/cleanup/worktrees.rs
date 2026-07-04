use crate::state::sqlite_store::mutations::{self, Result, SqliteStateStore};

impl SqliteStateStore {
	pub(in crate::state) fn delete_lease(&mut self, issue_id: &str) -> Result<()> {
		self.connection
			.execute("DELETE FROM leases WHERE issue_id = ?1", mutations::params![issue_id])?;

		Ok(())
	}

	pub(in crate::state) fn delete_worktree_mapping(&mut self, issue_id: &str) -> Result<()> {
		self.connection
			.execute("DELETE FROM worktrees WHERE issue_id = ?1", mutations::params![issue_id])?;

		Ok(())
	}
}
