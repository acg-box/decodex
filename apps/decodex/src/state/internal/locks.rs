#[cfg(unix)] use std::os::{fd::AsRawFd, unix::ffi::OsStrExt as _};
use std::{
	env,
	fs::{self, File, OpenOptions, TryLockError},
	io::{Error, ErrorKind, Read as _, Seek as _, SeekFrom, Write as _},
	path::{Path, PathBuf},
};

use libc::{F_GETFD, F_SETFD, FD_CLOEXEC};

use crate::{
	prelude::{Result, eyre},
	state::{DISPATCH_SLOT_LOCK_FILE_PREFIX, ISSUE_CLAIM_LOCK_FILE_PREFIX, IssueLease},
};

pub(in crate::state) fn dispatch_slot_lock_path(root: &Path, slot_index: usize) -> PathBuf {
	root.join(format!("{DISPATCH_SLOT_LOCK_FILE_PREFIX}.{slot_index}.lock"))
}

pub(in crate::state) fn issue_claim_lock_path(root: &Path, issue_id: &str) -> PathBuf {
	root.join(format!("{ISSUE_CLAIM_LOCK_FILE_PREFIX}.{issue_id}.lock"))
}

pub(in crate::state) fn issue_claim_id_from_path(path: &Path) -> Option<String> {
	let file_name = path.file_name()?.to_str()?;

	file_name
		.strip_prefix(&format!("{ISSUE_CLAIM_LOCK_FILE_PREFIX}."))
		.and_then(|suffix| suffix.strip_suffix(".lock"))
		.map(str::to_owned)
}

pub(in crate::state) fn shared_lock_coordinator_path(root: &Path) -> PathBuf {
	let mut hash = 0xcbf2_9ce4_8422_2325_u64;

	for byte in root.as_os_str().as_bytes() {
		hash ^= u64::from(*byte);
		hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
	}

	env::temp_dir().join("decodex-shared-lock-coordinators").join(format!("{hash:016x}.lock"))
}

pub(in crate::state) fn acquire_shared_lock_coordinator(root: &Path) -> Result<File> {
	fs::create_dir_all(root)?;

	let coordinator_path = shared_lock_coordinator_path(root);

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
	let _coordinator = acquire_shared_lock_coordinator(root)?;
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

pub(in crate::state) fn write_issue_claim_record(
	lock_file: &mut File,
	project_id: &str,
	issue_id: &str,
	run_id: &str,
	issue_state: &str,
) -> Result<()> {
	lock_file.set_len(0)?;
	lock_file.seek(SeekFrom::Start(0))?;

	write!(
		lock_file,
		"project_id={project_id}\nissue_id={issue_id}\nrun_id={run_id}\nissue_state={issue_state}\n"
	)?;

	lock_file.flush()?;

	Ok(())
}

pub(in crate::state) fn read_issue_claim_record(path: &Path) -> Result<Option<IssueLease>> {
	let mut body = String::new();
	let mut file = File::open(path)?;

	file.read_to_string(&mut body)?;

	if body.trim().is_empty() {
		return Ok(None);
	}

	let mut project_id = None;
	let mut issue_id = None;
	let mut run_id = None;
	let mut issue_state = None;

	for line in body.lines().filter(|line| !line.trim().is_empty()) {
		let (key, value) = line
			.split_once('=')
			.ok_or_else(|| eyre::eyre!("issue claim record `{}` is malformed", path.display()))?;

		match key {
			"project_id" => project_id = Some(value.to_owned()),
			"issue_id" => issue_id = Some(value.to_owned()),
			"run_id" => run_id = Some(value.to_owned()),
			"issue_state" => issue_state = Some(value.to_owned()),
			_ => {},
		}
	}

	let Some(project_id) = project_id else {
		return Err(eyre::eyre!("issue claim record `{}` is missing project_id", path.display()));
	};
	let Some(issue_id) = issue_id else {
		return Err(eyre::eyre!("issue claim record `{}` is missing issue_id", path.display()));
	};
	let Some(run_id) = run_id else {
		return Err(eyre::eyre!("issue claim record `{}` is missing run_id", path.display()));
	};
	let Some(issue_state) = issue_state else {
		return Err(eyre::eyre!("issue claim record `{}` is missing issue_state", path.display()));
	};

	Ok(Some(IssueLease { project_id, issue_id, run_id, issue_state }))
}

#[cfg(unix)]
pub(in crate::state) fn clear_close_on_exec(file: &File) -> Result<()> {
	let fd = file.as_raw_fd();
	let existing_flags = unsafe { libc::fcntl(fd, F_GETFD) };

	if existing_flags == -1 {
		return Err(Error::last_os_error().into());
	}

	let new_flags = existing_flags & !FD_CLOEXEC;

	if new_flags != existing_flags {
		let result = unsafe { libc::fcntl(fd, F_SETFD, new_flags) };

		if result == -1 {
			return Err(Error::last_os_error().into());
		}
	}

	Ok(())
}

#[cfg(unix)]
pub(in crate::state) fn set_close_on_exec(file: &File) -> Result<()> {
	let fd = file.as_raw_fd();
	let existing_flags = unsafe { libc::fcntl(fd, F_GETFD) };

	if existing_flags == -1 {
		return Err(Error::last_os_error().into());
	}

	let new_flags = existing_flags | FD_CLOEXEC;

	if new_flags != existing_flags {
		let result = unsafe { libc::fcntl(fd, F_SETFD, new_flags) };

		if result == -1 {
			return Err(Error::last_os_error().into());
		}
	}

	Ok(())
}
