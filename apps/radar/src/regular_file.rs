//! Bounded no-follow reads for external regular files.

use std::{
	fs::{File, Metadata, OpenOptions},
	io::Read as _,
	os::unix::fs::{MetadataExt as _, OpenOptionsExt as _},
	path::Path,
};

use crate::prelude::{Result, eyre};

#[derive(Clone, Debug, Eq, PartialEq)]
struct RegularFileIdentity {
	dev: u64,
	ino: u64,
	mtime_seconds: i64,
	mtime_nanoseconds: i64,
	ctime_seconds: i64,
	ctime_nanoseconds: i64,
	size: u64,
}

pub(crate) fn read_regular_file_bounded(
	path: &Path,
	max_bytes: u64,
	label: &str,
) -> Result<Vec<u8>> {
	read_regular_file_bounded_with(path, max_bytes, label, || {})
}

pub(crate) fn read_regular_file_bounded_with(
	path: &Path,
	max_bytes: u64,
	label: &str,
	after_metadata: impl FnOnce(),
) -> Result<Vec<u8>> {
	let (mut file, initial) = open_regular_file(path, label)?;

	if initial.size > max_bytes {
		eyre::bail!("{label} exceeds the bounded read limit");
	}
	after_metadata();

	let capacity = usize::try_from(initial.size)
		.map_err(|_| eyre::eyre!("{label} size cannot fit in memory"))?;
	let mut payload = Vec::with_capacity(capacity);
	let read_limit =
		max_bytes.checked_add(1).ok_or_else(|| eyre::eyre!("{label} read limit is too large"))?;

	file.by_ref().take(read_limit).read_to_end(&mut payload)?;
	if u64::try_from(payload.len()).unwrap_or(u64::MAX) > max_bytes {
		eyre::bail!("{label} exceeds the bounded read limit");
	}
	let final_identity = identity_from_metadata(&file.metadata()?);
	let (_, current_identity) = open_regular_file(path, label)?;

	if final_identity != initial || current_identity != initial {
		eyre::bail!("{label} identity changed during read");
	}

	Ok(payload)
}

fn open_regular_file(path: &Path, label: &str) -> Result<(File, RegularFileIdentity)> {
	let file = OpenOptions::new()
		.read(true)
		.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
		.open(path)
		.map_err(|error| {
			eyre::eyre!("{label} must be an accessible regular non-symlink file: {error}")
		})?;
	let metadata = file.metadata()?;

	if !metadata.is_file() {
		eyre::bail!("{label} must be a regular non-symlink file");
	}

	Ok((file, identity_from_metadata(&metadata)))
}

fn identity_from_metadata(metadata: &Metadata) -> RegularFileIdentity {
	RegularFileIdentity {
		dev: metadata.dev(),
		ino: metadata.ino(),
		mtime_seconds: metadata.mtime(),
		mtime_nanoseconds: metadata.mtime_nsec(),
		ctime_seconds: metadata.ctime(),
		ctime_nanoseconds: metadata.ctime_nsec(),
		size: metadata.len(),
	}
}
