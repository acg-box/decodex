#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::{fs, path::Path};

use crate::prelude::Result;

pub(in crate::accounts) fn secure_account_file(path: &Path) -> Result<()> {
	#[cfg(unix)]
	{
		let mode = if path.is_dir() { 0o700 } else { 0o600 };
		let mut permissions = fs::metadata(path)?.permissions();

		permissions.set_mode(mode);

		fs::set_permissions(path, permissions)?;
	}

	Ok(())
}
