use std::path::{Path, PathBuf};

pub(crate) const WORKTREE_PROVENANCE_FILESYSTEM_SCAN: &str = "filesystem_scan";
pub(crate) const WORKTREE_PROVENANCE_GIT_HYGIENE_SCAN: &str = "git_hygiene_scan";
pub(crate) const WORKTREE_PROVENANCE_LEGACY_UNKNOWN: &str = "legacy_unknown";
pub(crate) const WORKTREE_PROVENANCE_RUNTIME_RECOVERED: &str = "runtime_recovered";
pub(crate) const WORKTREE_PROVENANCE_RUNTIME_RECORDED: &str = "runtime_recorded";

/// Worktree mapping for one issue lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeMapping {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) branch_name: String,
	pub(in crate::state) worktree_path: PathBuf,
	pub(in crate::state) provenance: WorktreeProvenance,
}
impl WorktreeMapping {
	/// Local project identifier owning this lane.
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Issue identifier for this lane.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Branch name used for the lane.
	pub fn branch_name(&self) -> &str {
		&self.branch_name
	}

	/// Filesystem path to the worktree checkout.
	pub fn worktree_path(&self) -> &Path {
		&self.worktree_path
	}

	/// Durable provenance captured when Decodex recorded or migrated this mapping.
	pub fn provenance(&self) -> &WorktreeProvenance {
		&self.provenance
	}
}

/// Durable provenance for a retained worktree mapping.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeProvenance {
	pub(in crate::state) source: String,
	pub(in crate::state) created_at_unix: Option<i64>,
	pub(in crate::state) updated_at_unix: Option<i64>,
}
impl WorktreeProvenance {
	/// Source that created or last classified this mapping.
	pub fn source(&self) -> &str {
		&self.source
	}

	/// Unix timestamp for when this mapping was first recorded, when available.
	pub fn created_at_unix(&self) -> Option<i64> {
		self.created_at_unix
	}

	/// Unix timestamp for when this mapping was last refreshed, when available.
	pub fn updated_at_unix(&self) -> Option<i64> {
		self.updated_at_unix
	}

	/// Whether this mapping was migrated from a legacy row without durable provenance.
	pub fn is_legacy_unknown(&self) -> bool {
		self.source == WORKTREE_PROVENANCE_LEGACY_UNKNOWN
	}
}

pub(crate) fn worktree_provenance(
	source: impl Into<String>,
	created_at_unix: Option<i64>,
	updated_at_unix: Option<i64>,
) -> WorktreeProvenance {
	WorktreeProvenance { source: source.into(), created_at_unix, updated_at_unix }
}
