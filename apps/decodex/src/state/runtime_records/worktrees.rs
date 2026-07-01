use std::path::PathBuf;

use crate::state::{self, WorktreeMapping};

#[derive(Clone, Debug)]
pub(in crate::state) struct WorktreeMappingRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) branch_name: String,
	pub(in crate::state) worktree_path: PathBuf,
	pub(in crate::state) provenance_source: String,
	pub(in crate::state) created_at_unix: Option<i64>,
	pub(in crate::state) updated_at_unix: Option<i64>,
}
impl WorktreeMappingRecord {
	pub(in crate::state) fn as_public(&self) -> WorktreeMapping {
		WorktreeMapping {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			branch_name: self.branch_name.clone(),
			worktree_path: self.worktree_path.clone(),
			provenance: state::worktree_provenance(
				self.provenance_source.clone(),
				self.created_at_unix,
				self.updated_at_unix,
			),
		}
	}
}
