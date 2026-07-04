use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MergedWorktreeCleanupDebt {
	pub(crate) branch_name: String,
	pub(crate) cleanliness: MergedWorktreeCleanliness,
	pub(crate) default_branch: String,
	pub(crate) path: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MergedWorktreeCleanliness {
	Clean,
	Dirty,
}
impl MergedWorktreeCleanliness {
	pub(crate) fn is_dirty(self) -> bool {
		self == Self::Dirty
	}
}
