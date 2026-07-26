#[cfg(unix)] use std::os::unix::fs::OpenOptionsExt as _;
use std::{
	fs::{self, File, OpenOptions},
	io::Write as _,
	process,
};

use crate::{
	accounts::secure_account_file,
	agent::codex_accounts::{AccountPoolRecord, CodexAccountPool, record},
	prelude::{Result, eyre},
};

impl CodexAccountPool {
	pub(super) fn load_records(&self) -> Result<Vec<AccountPoolRecord>> {
		let input = fs::read_to_string(&self.path).map_err(|error| {
			eyre::eyre!("Failed to read Codex accounts `{}`: {error}", self.path.display())
		})?;

		record::parse_account_records(&input, &self.path)
	}

	pub(super) fn lock_records(&self) -> Result<AccountPoolFileLock> {
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
		let mut options = OpenOptions::new();

		options.read(true).write(true).create(true).truncate(false);
		#[cfg(unix)]
		options.mode(0o600);

		let file = options.open(&lock_path).map_err(|error| {
			eyre::eyre!("Failed to open Codex accounts lock `{}`: {error}", lock_path.display())
		})?;

		secure_account_file(&lock_path)?;

		file.lock().map_err(|error| {
			eyre::eyre!("Failed to lock Codex accounts `{}`: {error}", self.path.display())
		})?;

		Ok(AccountPoolFileLock { _file: file })
	}

	pub(super) fn save_records(&self, records: &[AccountPoolRecord]) -> Result<()> {
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

		let mut options = OpenOptions::new();

		options.write(true).create(true).truncate(true);
		#[cfg(unix)]
		options.mode(0o600);

		let mut temp_file = options.open(&temp_path)?;

		secure_account_file(&temp_path)?;
		temp_file.write_all(body.as_bytes())?;
		drop(temp_file);
		secure_account_file(&temp_path)?;
		fs::rename(temp_path, &self.path)?;
		secure_account_file(&self.path)?;

		Ok(())
	}
}

pub(super) struct AccountPoolFileLock {
	_file: File,
}

#[cfg(all(test, unix))]
mod tests {
	use std::{
		fs,
		os::unix::fs::{MetadataExt as _, PermissionsExt as _},
		path::Path,
		thread,
		time::Duration,
	};

	use tempfile::TempDir;

	use super::*;
	use crate::agent::codex_accounts::{
		DEFAULT_REFRESH_ENDPOINT, DEFAULT_RESET_CREDITS_ENDPOINT, DEFAULT_USAGE_ENDPOINT,
	};

	#[test]
	fn writer_secures_new_and_reused_temp_file_before_atomic_replace() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let accounts_path = temp_dir.path().join("accounts.jsonl");
		let staged_path = temp_dir.path().join(format!(".accounts.jsonl.tmp-{}", process::id()));

		fs::create_dir(&accounts_path).expect("blocking accounts directory should exist");

		let pool = account_pool(&accounts_path);

		let _error = pool.save_records(&[]).expect_err("replace over a directory should fail");
		assert_eq!(file_mode(&staged_path), 0o600);

		set_mode(&staged_path, 0o666);
		let _error =
			pool.save_records(&[]).expect_err("replace over a directory should still fail");
		assert_eq!(file_mode(&staged_path), 0o600);
	}

	#[test]
	fn writer_secures_accounts_file_after_every_atomic_replace() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let accounts_path = temp_dir.path().join("accounts.jsonl");
		let pool = account_pool(&accounts_path);

		pool.save_records(&[]).expect("initial accounts write should succeed");
		assert_eq!(file_mode(&accounts_path), 0o600);

		set_mode(&accounts_path, 0o666);
		pool.save_records(&[]).expect("replacement accounts write should succeed");
		assert_eq!(file_mode(&accounts_path), 0o600);
	}

	#[test]
	fn lock_file_is_owner_only_when_created_and_reopened() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let accounts_path = temp_dir.path().join("accounts.jsonl");
		let lock_path = temp_dir.path().join(".accounts.jsonl.lock");
		let pool = account_pool(&accounts_path);

		let lock = pool.lock_records().expect("lock file should open");

		assert_eq!(file_mode(&lock_path), 0o600);

		drop(lock);
		set_mode(&lock_path, 0o666);

		let _lock = pool.lock_records().expect("existing lock file should reopen");

		assert_eq!(file_mode(&lock_path), 0o600);
	}

	#[test]
	fn reopening_secure_lock_does_not_change_its_metadata() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let accounts_path = temp_dir.path().join("accounts.jsonl");
		let lock_path = temp_dir.path().join(".accounts.jsonl.lock");
		let pool = account_pool(&accounts_path);

		drop(pool.lock_records().expect("lock file should open"));
		let before = changed_time(&lock_path);

		thread::sleep(Duration::from_millis(20));
		drop(pool.lock_records().expect("secure lock file should reopen"));

		assert_eq!(changed_time(&lock_path), before);
	}

	fn account_pool(path: &Path) -> CodexAccountPool {
		CodexAccountPool::new_with_fixed_account(
			path,
			DEFAULT_USAGE_ENDPOINT,
			DEFAULT_RESET_CREDITS_ENDPOINT,
			DEFAULT_REFRESH_ENDPOINT,
			None,
		)
		.expect("account pool should initialize")
	}

	fn set_mode(path: &Path, mode: u32) {
		let mut permissions = fs::metadata(path).expect("file metadata should exist").permissions();

		permissions.set_mode(mode);
		fs::set_permissions(path, permissions).expect("file permissions should update");
	}

	fn file_mode(path: &Path) -> u32 {
		fs::metadata(path).expect("file metadata should exist").permissions().mode() & 0o777
	}

	fn changed_time(path: &Path) -> (i64, i64) {
		let metadata = fs::metadata(path).expect("file metadata should exist");

		(metadata.ctime(), metadata.ctime_nsec())
	}
}
