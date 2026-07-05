use std::{
	fs::{self, OpenOptions, TryLockError},
	io::ErrorKind,
	path::Path,
};

use crate::{
	prelude::Result,
	state::{DISPATCH_SLOT_LOCK_FILE_PREFIX, ISSUE_CLAIM_LOCK_FILE_PREFIX, internal::locks},
};

pub(in crate::state) fn remove_lock_file_if_exists(path: &Path) -> Result<()> {
	match fs::remove_file(path) {
		Ok(()) => Ok(()),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
		Err(error) => Err(error.into()),
	}
}

pub(in crate::state) fn shared_lock_file_is_cleanup_candidate(path: &Path) -> bool {
	let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
		return false;
	};

	file_name.starts_with(&format!("{ISSUE_CLAIM_LOCK_FILE_PREFIX}."))
		|| file_name.starts_with(&format!("{DISPATCH_SLOT_LOCK_FILE_PREFIX}."))
}

pub(in crate::state) fn prune_unlocked_shared_lock_files(root: &Path) -> Result<()> {
	let _coordinator = locks::acquire_shared_lock_coordinator(root)?;
	let read_dir = match fs::read_dir(root) {
		Ok(read_dir) => read_dir,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
		Err(error) => return Err(error.into()),
	};

	for entry in read_dir {
		let path = entry?.path();

		if !shared_lock_file_is_cleanup_candidate(&path) {
			continue;
		}

		let lock_file = match OpenOptions::new()
			.read(true)
			.write(true)
			.create(false)
			.truncate(false)
			.open(&path)
		{
			Ok(file) => file,
			Err(error) if error.kind() == ErrorKind::NotFound => continue,
			Err(error) => return Err(error.into()),
		};

		match lock_file.try_lock() {
			Ok(()) => {
				lock_file.unlock()?;

				drop(lock_file);
				remove_lock_file_if_exists(&path)?;
			},
			Err(TryLockError::WouldBlock) => {},
			Err(TryLockError::Error(error)) => return Err(error.into()),
		}
	}

	Ok(())
}
