use std::{fs, path::PathBuf};

#[derive(Debug)]
pub(in crate::release_delta::backfill) struct PreparedReleaseDelta {
	pub(in crate::release_delta::backfill) path: PathBuf,
	pub(in crate::release_delta::backfill) cleanup_dir: Option<PathBuf>,
}
impl Drop for PreparedReleaseDelta {
	fn drop(&mut self) {
		if let Some(path) = &self.cleanup_dir {
			let _ = fs::remove_dir_all(path);
		}
	}
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::release_delta::backfill) struct ReleaseSelection {
	pub(in crate::release_delta::backfill) stable_tag: String,
	pub(in crate::release_delta::backfill) preview_tag: String,
	pub(in crate::release_delta::backfill) pr_numbers: Vec<u64>,
}

#[derive(Debug)]
pub(in crate::release_delta::backfill) struct BackfillPaths {
	pub(in crate::release_delta::backfill) bundle: PathBuf,
	pub(in crate::release_delta::backfill) analysis: PathBuf,
	pub(in crate::release_delta::backfill) signal: PathBuf,
}
