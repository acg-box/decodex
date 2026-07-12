use crate::state::sqlite_store::schema::{Result, SqliteStateStore};

impl SqliteStateStore {
	pub(in crate::state) fn bootstrap_lane_schema(&self) -> Result<()> {
		self.ensure_column(
			"lanes",
			"intake_authority_id",
			"ALTER TABLE lanes ADD COLUMN intake_authority_id TEXT",
		)
	}

	#[cfg(test)]
	pub(in crate::state) fn bootstrap_worktree_schema(&self) -> Result<()> {
		self.ensure_column(
			"worktrees",
			"provenance_source",
			"ALTER TABLE worktrees ADD COLUMN provenance_source TEXT NOT NULL DEFAULT 'legacy_unknown'",
		)?;
		self.ensure_column(
			"worktrees",
			"created_at_unix",
			"ALTER TABLE worktrees ADD COLUMN created_at_unix INTEGER",
		)?;
		self.ensure_column(
			"worktrees",
			"updated_at_unix",
			"ALTER TABLE worktrees ADD COLUMN updated_at_unix INTEGER",
		)?;

		Ok(())
	}
}
