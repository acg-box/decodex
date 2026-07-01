use std::{
	fs::{self, File, OpenOptions},
	process,
};

use crate::prelude::eyre;

use super::{AccountPoolRecord, CodexAccountPool, record::parse_account_records};

impl CodexAccountPool {
	pub(super) fn load_records(&self) -> crate::prelude::Result<Vec<AccountPoolRecord>> {
		let input = fs::read_to_string(&self.path).map_err(|error| {
			eyre::eyre!("Failed to read Codex accounts `{}`: {error}", self.path.display())
		})?;

		parse_account_records(&input, &self.path)
	}

	pub(super) fn lock_records(&self) -> crate::prelude::Result<AccountPoolFileLock> {
		let parent = self.path.parent().ok_or_else(|| {
			eyre::eyre!(
				"Codex accounts path `{}` must have a parent directory.",
				self.path.display()
			)
		})?;
		let file_name = self
			.path
			.file_name()
			.and_then(|name| name.to_str())
			.ok_or_else(|| eyre::eyre!("Codex accounts path must end in a valid file name."))?;
		let lock_path = parent.join(format!(".{file_name}.lock"));
		let file = OpenOptions::new()
			.read(true)
			.write(true)
			.create(true)
			.truncate(false)
			.open(&lock_path)
			.map_err(|error| {
				eyre::eyre!("Failed to open Codex accounts lock `{}`: {error}", lock_path.display())
			})?;

		file.lock().map_err(|error| {
			eyre::eyre!("Failed to lock Codex accounts `{}`: {error}", self.path.display())
		})?;

		Ok(AccountPoolFileLock { _file: file })
	}

	pub(super) fn save_records(&self, records: &[AccountPoolRecord]) -> crate::prelude::Result<()> {
		let parent = self.path.parent().ok_or_else(|| {
			eyre::eyre!(
				"Codex accounts path `{}` must have a parent directory.",
				self.path.display()
			)
		})?;
		let file_name = self
			.path
			.file_name()
			.and_then(|name| name.to_str())
			.ok_or_else(|| eyre::eyre!("Codex accounts path must end in a valid file name."))?;
		let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
		let mut body = String::new();

		for record in records {
			body.push_str(&serde_json::to_string(record)?);
			body.push('\n');
		}

		fs::write(&temp_path, body)?;
		fs::rename(temp_path, &self.path)?;

		Ok(())
	}
}

pub(super) struct AccountPoolFileLock {
	_file: File,
}
