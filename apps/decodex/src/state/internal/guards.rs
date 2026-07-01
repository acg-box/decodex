use std::{
	fs::File,
	path::{Path, PathBuf},
};

use crate::{
	prelude::Result,
	state::{internal::locks, runtime_records::GuardRetention},
};

#[derive(Clone)]
pub(in crate::state) struct DispatchSlotConfig {
	pub(in crate::state) root: PathBuf,
}

pub(in crate::state) struct IssueClaimGuard {
	pub(in crate::state) lock_path: PathBuf,
	pub(in crate::state) lock_file: File,
	pub(in crate::state) retention: GuardRetention,
}
impl IssueClaimGuard {
	pub(in crate::state) fn lock_root(&self) -> Result<&Path> {
		locks::lock_root_from_lock_path(&self.lock_path)
	}

	pub(in crate::state) fn unlock(self) -> Result<()> {
		let Self { lock_path, lock_file, retention: _ } = self;

		lock_file.unlock()?;

		drop(lock_file);

		locks::remove_lock_file_if_exists(&lock_path)?;

		Ok(())
	}

	pub(in crate::state) fn release_for_clear(self) -> Result<()> {
		match self.retention {
			GuardRetention::ParentAfterHandoff => Ok(()),
			GuardRetention::Local | GuardRetention::AdoptingChild => self.unlock(),
		}
	}
}

pub(in crate::state) struct DispatchSlotGuard {
	pub(in crate::state) project_id: String,
	pub(in crate::state) slot_index: usize,
	pub(in crate::state) lock_path: PathBuf,
	pub(in crate::state) lock_file: File,
	pub(in crate::state) retention: GuardRetention,
}
impl DispatchSlotGuard {
	pub(in crate::state) fn lock_root(&self) -> Result<&Path> {
		locks::lock_root_from_lock_path(&self.lock_path)
	}

	pub(in crate::state) fn release_for_clear(self) -> Result<()> {
		match self.retention {
			GuardRetention::ParentAfterHandoff => Ok(()),
			GuardRetention::Local | GuardRetention::AdoptingChild => {
				let Self { project_id: _, slot_index: _, lock_path, lock_file, retention: _ } =
					self;

				lock_file.unlock()?;

				drop(lock_file);

				locks::remove_lock_file_if_exists(&lock_path)?;

				Ok(())
			},
		}
	}
}
