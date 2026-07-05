use std::{
	fs::{self, File, OpenOptions},
	path::Path,
};

use crate::{
	prelude::{Result, eyre},
	state::internal::locks,
};

pub(in crate::state) fn acquire_shared_lock_coordinator(root: &Path) -> Result<File> {
	fs::create_dir_all(root)?;

	let coordinator_path = locks::shared_lock_coordinator_path(root);

	if let Some(parent) = coordinator_path.parent() {
		fs::create_dir_all(parent)?;
	}

	let coordinator = OpenOptions::new()
		.read(true)
		.write(true)
		.create(true)
		.truncate(false)
		.open(coordinator_path)?;

	coordinator.lock()?;

	Ok(coordinator)
}

pub(in crate::state) fn lock_root_from_lock_path(lock_path: &Path) -> Result<&Path> {
	lock_path
		.parent()
		.ok_or_else(|| eyre::eyre!("shared lock path `{}` has no parent root", lock_path.display()))
}
