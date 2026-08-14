//! Descriptor-anchored Unix filesystem operations for Decodex-owned state.
//!
//! Every component is opened relative to an already validated directory descriptor.
//! Final operations therefore cannot be redirected by swapping an ancestor path for a
//! symbolic link between validation and I/O.

use std::{
	ffi::{CStr, CString, OsStr, OsString},
	fs::{File, Metadata},
	io::{self, ErrorKind, Read, Write},
	mem::MaybeUninit,
	os::{
		fd::{AsRawFd as _, FromRawFd as _, RawFd},
		unix::{
			ffi::{OsStrExt as _, OsStringExt as _},
			fs::MetadataExt as _,
		},
	},
	path::{Component, Path, PathBuf},
};

use libc::{
	AT_SYMLINK_NOFOLLOW, DIR, F_DUPFD_CLOEXEC, O_CLOEXEC, O_CREAT, O_DIRECTORY, O_EXCL, O_NOFOLLOW,
	O_NONBLOCK, O_RDONLY, O_RDWR, O_WRONLY, S_IFDIR, S_IFLNK, S_IFMT, S_IFREG, c_int, c_uint,
	mode_t, stat, uid_t,
};

use crate::{
	DecodexPaths, PathError,
	paths::{self, AtomicMode, IoOperation, PRIVATE_DIRECTORY_MODE, PRIVATE_FILE_MODE},
};

const ROOT_PATH: &[u8] = b"/\0";

#[cfg(target_vendor = "apple")]
const TRAVERSAL_DIRECTORY_ACCESS: c_int = libc::O_SEARCH;
#[cfg(not(target_vendor = "apple"))]
const TRAVERSAL_DIRECTORY_ACCESS: c_int = O_RDONLY;

#[derive(Clone, Copy, Eq, PartialEq)]
enum ExpectedKind {
	Directory,
	File,
}

struct DirectoryStream(*mut DIR);
impl DirectoryStream {
	fn open(directory: &File) -> Result<Self, PathError> {
		// SAFETY: `fcntl` duplicates an open descriptor and does not retain pointers.
		let descriptor = unsafe { libc::fcntl(directory.as_raw_fd(), F_DUPFD_CLOEXEC, 0) };

		if descriptor == -1 {
			return Err(paths::io_error(IoOperation::List, io::Error::last_os_error()));
		}

		// SAFETY: `descriptor` is a fresh directory descriptor. On success, `fdopendir`
		// assumes ownership; on failure, this function closes it below.
		let stream = unsafe { libc::fdopendir(descriptor) };

		if stream.is_null() {
			let error = io::Error::last_os_error();

			// SAFETY: ownership was not transferred when `fdopendir` returned null.
			unsafe { libc::close(descriptor) };

			return Err(paths::io_error(IoOperation::List, error));
		}

		Ok(Self(stream))
	}

	fn next_name(&mut self) -> Result<Option<OsString>, PathError> {
		loop {
			clear_errno();

			// SAFETY: `self.0` remains an open `DIR` until `Drop`; `readdir` owns the
			// returned entry storage until the next call on this stream.
			let entry = unsafe { libc::readdir(self.0) };

			if entry.is_null() {
				let error = current_errno();

				return if error == 0 {
					Ok(None)
				} else {
					Err(paths::io_error(IoOperation::List, io::Error::from_raw_os_error(error)))
				};
			}

			// SAFETY: a non-null `dirent` from `readdir` contains a NUL-terminated
			// `d_name` valid until the next call on this stream.
			let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();

			if matches!(bytes, b"." | b"..") {
				continue;
			}

			return Ok(Some(OsString::from_vec(bytes.to_vec())));
		}
	}
}

impl Drop for DirectoryStream {
	fn drop(&mut self) {
		// SAFETY: `self.0` is a non-null `DIR` uniquely owned by this guard.
		unsafe { libc::closedir(self.0) };
	}
}

pub(crate) fn ensure_layout(paths: &DecodexPaths) -> Result<(), PathError> {
	open_root(paths, true)?;

	for relative in ["logs", "blobs", "blobs/sha256", "cache", "server"] {
		ensure_owned_directory(paths, Path::new(relative))?;
	}

	Ok(())
}

pub(crate) fn ensure_owned_directory(
	paths: &DecodexPaths,
	relative: &Path,
) -> Result<PathBuf, PathError> {
	paths::validate_relative(relative)?;

	let mut directory = open_root(paths, true)?;
	let mut result = paths.root().as_path().to_path_buf();

	for component in relative.components() {
		let Component::Normal(name) = component else {
			return Err(PathError::Escape);
		};

		directory = ensure_private_directory_at(&directory, name)?;

		result.push(name);
	}

	Ok(result)
}

pub(crate) fn read_private_file(
	paths: &DecodexPaths,
	path: &Path,
	maximum_bytes: usize,
) -> Result<Vec<u8>, PathError> {
	let (parent, name) = open_file_parent(paths, path)?;
	let file = open_private_file_at(&parent, &name)?;
	let metadata = file.metadata().map_err(|error| paths::io_error(IoOperation::Inspect, error))?;

	if metadata.len() > u64::try_from(maximum_bytes).unwrap_or(u64::MAX) {
		return Err(PathError::Oversized { limit: maximum_bytes });
	}

	let mut bytes = Vec::with_capacity(metadata.len().min(maximum_bytes as u64) as usize);

	file.take((maximum_bytes as u64).saturating_add(1))
		.read_to_end(&mut bytes)
		.map_err(|error| paths::io_error(IoOperation::Read, error))?;

	if bytes.len() > maximum_bytes {
		return Err(PathError::Oversized { limit: maximum_bytes });
	}

	Ok(bytes)
}

pub(crate) fn open_private_database_file(
	paths: &DecodexPaths,
	path: &Path,
) -> Result<File, PathError> {
	let (parent, name) = open_file_parent(paths, path)?;
	let name = c_name(&name)?;
	let mut created = false;

	let descriptor = match open_private_database_file_at(&parent, &name, false) {
		Ok(descriptor) => descriptor,
		Err(PathError::Io { kind: ErrorKind::NotFound, .. }) => {
			match open_private_database_file_at(&parent, &name, true) {
				Ok(descriptor) => {
					created = true;
					descriptor
				},
				Err(PathError::Io { kind: ErrorKind::AlreadyExists, .. }) =>
					open_private_database_file_at(&parent, &name, false)?,
				Err(error) => return Err(error),
			}
		},
		Err(error) => return Err(error),
	};
	let file = file_from_descriptor(descriptor, IoOperation::Open)?;

	verify_private_database_file_metadata(
		&file.metadata().map_err(|error| paths::io_error(IoOperation::Inspect, error))?,
	)?;
	if created {
		parent.sync_all().map_err(|error| paths::io_error(IoOperation::Sync, error))?;
	}

	Ok(file)
}

pub(crate) fn atomic_write(
	paths: &DecodexPaths,
	path: &Path,
	bytes: &[u8],
	maximum_bytes: usize,
	mode: AtomicMode,
) -> Result<(), PathError> {
	if bytes.len() > maximum_bytes {
		return Err(PathError::Oversized { limit: maximum_bytes });
	}

	let (parent, target_name) = open_file_parent(paths, path)?;

	match open_private_file_at(&parent, &target_name) {
		Ok(_) if mode == AtomicMode::CreateOnly => return Err(PathError::AlreadyExists),
		Ok(_) => {},
		Err(PathError::Io { kind: ErrorKind::NotFound, .. }) => {},
		Err(error) => return Err(error),
	}

	let (temporary_name, mut temporary_file) = create_temporary_file(&parent)?;
	let result = (|| {
		temporary_file
			.write_all(bytes)
			.map_err(|error| paths::io_error(IoOperation::Write, error))?;
		temporary_file.sync_all().map_err(|error| paths::io_error(IoOperation::Sync, error))?;

		verify_private_file_metadata(
			&temporary_file
				.metadata()
				.map_err(|error| paths::io_error(IoOperation::Inspect, error))?,
		)?;

		match mode {
			AtomicMode::Replace => rename_at(&parent, &temporary_name, &target_name)?,
			AtomicMode::CreateOnly => {
				link_at(&parent, &temporary_name, &target_name)?;
				unlink_at(&parent, &temporary_name)?;
			},
		}

		parent.sync_all().map_err(|error| paths::io_error(IoOperation::Sync, error))
	})();

	if result.is_err() {
		let _ = unlink_at(&parent, &temporary_name);
	}

	result
}

pub(crate) fn remove_private_file(paths: &DecodexPaths, path: &Path) -> Result<(), PathError> {
	let (parent, name) = open_file_parent(paths, path)?;
	let file = open_private_file_at(&parent, &name)?;

	unlink_at(&parent, &name)?;
	drop(file);

	parent.sync_all().map_err(|error| paths::io_error(IoOperation::Sync, error))
}

pub(crate) fn visit_private_files<E>(
	paths: &DecodexPaths,
	directory: &Path,
	mut visitor: impl FnMut(PathBuf, Metadata) -> Result<(), E>,
) -> Result<(), E>
where
	E: From<PathError>,
{
	let directory_file = open_owned_directory(paths, directory).map_err(E::from)?;
	let mut stream = DirectoryStream::open(&directory_file).map_err(E::from)?;

	while let Some(name) = stream.next_name().map_err(E::from)? {
		let file = open_private_file_at(&directory_file, &name).map_err(E::from)?;
		let metadata = file
			.metadata()
			.map_err(|error| E::from(paths::io_error(IoOperation::Inspect, error)))?;

		visitor(directory.join(name), metadata)?;
	}

	Ok(())
}

fn open_root(paths: &DecodexPaths, create: bool) -> Result<File, PathError> {
	let root = paths.root().as_path();
	let components = root
		.components()
		.filter_map(|component| match component {
			Component::Normal(name) => Some(name),
			_ => None,
		})
		.collect::<Vec<_>>();
	let Some((last, ancestors)) = components.split_last() else {
		return Err(PathError::UnsafeRoot);
	};
	let mut directory = open_filesystem_root()?;

	for name in ancestors {
		directory = open_traversal_directory_at(&directory, name)?;
	}

	let root = if create {
		ensure_private_directory_at(&directory, last)?
	} else {
		let root = open_directory_at(&directory, last)?;

		verify_private_directory_metadata(
			&root.metadata().map_err(|error| paths::io_error(IoOperation::Inspect, error))?,
		)?;

		root
	};

	Ok(root)
}

fn open_filesystem_root() -> Result<File, PathError> {
	let path = CStr::from_bytes_with_nul(ROOT_PATH).map_err(|_| PathError::UnsafeRoot)?;
	// SAFETY: `path` is a valid NUL-terminated C string and successful `open` returns
	// a new descriptor owned by the caller.
	let descriptor =
		unsafe { libc::open(path.as_ptr(), TRAVERSAL_DIRECTORY_ACCESS | O_DIRECTORY | O_CLOEXEC) };

	file_from_descriptor(descriptor, IoOperation::Open)
}

fn open_owned_directory(paths: &DecodexPaths, path: &Path) -> Result<File, PathError> {
	let relative = path.strip_prefix(paths.root().as_path()).map_err(|_| PathError::Escape)?;

	paths::validate_relative(relative)?;

	let mut directory = open_root(paths, false)?;

	for component in relative.components() {
		let Component::Normal(name) = component else {
			return Err(PathError::Escape);
		};

		directory = open_directory_at(&directory, name)?;

		verify_private_directory_metadata(
			&directory.metadata().map_err(|error| paths::io_error(IoOperation::Inspect, error))?,
		)?;
	}

	Ok(directory)
}

fn open_file_parent(paths: &DecodexPaths, path: &Path) -> Result<(File, OsString), PathError> {
	let relative = path.strip_prefix(paths.root().as_path()).map_err(|_| PathError::Escape)?;

	paths::validate_relative(relative)?;

	let mut components = relative.components().peekable();
	let mut directory = open_root(paths, false)?;

	while let Some(component) = components.next() {
		let Component::Normal(name) = component else {
			return Err(PathError::Escape);
		};

		if components.peek().is_none() {
			return Ok((directory, name.to_os_string()));
		}

		directory = open_directory_at(&directory, name)?;

		verify_private_directory_metadata(
			&directory.metadata().map_err(|error| paths::io_error(IoOperation::Inspect, error))?,
		)?;
	}

	Err(PathError::Escape)
}

fn ensure_private_directory_at(parent: &File, name: &OsStr) -> Result<File, PathError> {
	let name = c_name(name)?;

	match open_directory_at_c(parent, &name) {
		Ok(directory) => {
			verify_private_directory_metadata(
				&directory
					.metadata()
					.map_err(|error| paths::io_error(IoOperation::Inspect, error))?,
			)?;

			Ok(directory)
		},
		Err(PathError::Io { kind: ErrorKind::NotFound, .. }) => {
			// SAFETY: `parent` is open, `name` is NUL-terminated, and `mkdirat` does
			// not retain either pointer.
			let result = unsafe {
				libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), PRIVATE_DIRECTORY_MODE as mode_t)
			};

			if result == -1 {
				let error = io::Error::last_os_error();

				if error.kind() != ErrorKind::AlreadyExists {
					return Err(paths::io_error(IoOperation::CreateDirectory, error));
				}
			}

			let directory = open_directory_at_c(parent, &name)?;

			verify_private_directory_metadata(
				&directory
					.metadata()
					.map_err(|error| paths::io_error(IoOperation::Inspect, error))?,
			)?;

			Ok(directory)
		},
		Err(error) => Err(error),
	}
}

fn open_directory_at(parent: &File, name: &OsStr) -> Result<File, PathError> {
	open_directory_at_c(parent, &c_name(name)?)
}

fn open_traversal_directory_at(parent: &File, name: &OsStr) -> Result<File, PathError> {
	open_directory_at_c_with_access(parent, &c_name(name)?, TRAVERSAL_DIRECTORY_ACCESS)
}

fn open_directory_at_c(parent: &File, name: &CStr) -> Result<File, PathError> {
	open_directory_at_c_with_access(parent, name, O_RDONLY)
}

fn open_directory_at_c_with_access(
	parent: &File,
	name: &CStr,
	access: c_int,
) -> Result<File, PathError> {
	// SAFETY: `parent` is an open directory, `name` is NUL-terminated, and a
	// successful `openat` returns a new descriptor owned by the caller.
	let descriptor = unsafe {
		libc::openat(
			parent.as_raw_fd(),
			name.as_ptr(),
			access | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
		)
	};

	if descriptor == -1 {
		let error = io::Error::last_os_error();

		return Err(classify_at_error(parent, name, error, ExpectedKind::Directory));
	}

	file_from_descriptor(descriptor, IoOperation::Open)
}

fn open_private_file_at(parent: &File, name: &OsStr) -> Result<File, PathError> {
	let name = c_name(name)?;
	// `O_NONBLOCK` prevents a hostile FIFO in a file position from blocking before
	// `fstat` can reject its kind.
	// SAFETY: `parent` is an open directory, `name` is NUL-terminated, and a
	// successful `openat` returns a new descriptor owned by the caller.
	let descriptor = unsafe {
		libc::openat(
			parent.as_raw_fd(),
			name.as_ptr(),
			O_RDONLY | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC,
		)
	};

	if descriptor == -1 {
		let error = io::Error::last_os_error();

		return Err(classify_at_error(parent, &name, error, ExpectedKind::File));
	}

	let file = file_from_descriptor(descriptor, IoOperation::Open)?;

	verify_private_file_metadata(
		&file.metadata().map_err(|error| paths::io_error(IoOperation::Inspect, error))?,
	)?;

	Ok(file)
}

fn open_private_database_file_at(
	parent: &File,
	name: &CStr,
	create: bool,
) -> Result<RawFd, PathError> {
	let flags = O_RDWR
		| O_NONBLOCK
		| O_NOFOLLOW
		| O_CLOEXEC
		| if create { O_CREAT | O_EXCL } else { 0 };
	// SAFETY: `parent` is open, `name` is NUL-terminated, and a successful
	// `openat` returns one new descriptor owned by the caller.
	let descriptor = unsafe {
		libc::openat(parent.as_raw_fd(), name.as_ptr(), flags, PRIVATE_FILE_MODE as c_uint)
	};

	if descriptor == -1 {
		let error = io::Error::last_os_error();

		return Err(classify_at_error(parent, name, error, ExpectedKind::File));
	}

	Ok(descriptor)
}

fn create_temporary_file(parent: &File) -> Result<(OsString, File), PathError> {
	for _ in 0..8 {
		let mut random = [0_u8; 16];

		getrandom::fill(&mut random).map_err(|_| PathError::RandomnessUnavailable)?;

		let name = OsString::from(format!("{}{}", paths::ATOMIC_TEMPORARY_PREFIX, hex(&random)));
		let c_name = c_name(&name)?;
		// SAFETY: `parent` is an open directory, `c_name` is NUL-terminated, and a
		// successful `openat` returns a new descriptor owned by the caller.
		let descriptor = unsafe {
			libc::openat(
				parent.as_raw_fd(),
				c_name.as_ptr(),
				O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
				PRIVATE_FILE_MODE as c_uint,
			)
		};

		if descriptor != -1 {
			return file_from_descriptor(descriptor, IoOperation::Open).map(|file| (name, file));
		}

		let error = io::Error::last_os_error();

		if error.kind() != ErrorKind::AlreadyExists {
			return Err(paths::io_error(IoOperation::Open, error));
		}
	}

	Err(PathError::AlreadyExists)
}

fn rename_at(parent: &File, source: &OsStr, target: &OsStr) -> Result<(), PathError> {
	let source = c_name(source)?;
	let target = c_name(target)?;
	// SAFETY: both names are valid C strings and both are resolved relative to the
	// same open directory descriptor.
	let result = unsafe {
		libc::renameat(parent.as_raw_fd(), source.as_ptr(), parent.as_raw_fd(), target.as_ptr())
	};

	zero_result(result, IoOperation::Rename)
}

fn link_at(parent: &File, source: &OsStr, target: &OsStr) -> Result<(), PathError> {
	let source = c_name(source)?;
	let target = c_name(target)?;
	// SAFETY: both names are valid C strings and both are resolved relative to the
	// same open directory descriptor. Flags are zero, so no symbolic link is followed.
	let result = unsafe {
		libc::linkat(parent.as_raw_fd(), source.as_ptr(), parent.as_raw_fd(), target.as_ptr(), 0)
	};

	if result == -1 {
		let error = io::Error::last_os_error();

		if error.kind() == ErrorKind::AlreadyExists {
			return Err(PathError::AlreadyExists);
		}

		return Err(paths::io_error(IoOperation::Link, error));
	}

	Ok(())
}

fn unlink_at(parent: &File, name: &OsStr) -> Result<(), PathError> {
	let name = c_name(name)?;
	// SAFETY: `name` is a valid C string resolved relative to the open directory;
	// flags are zero, so only a non-directory entry itself is removed.
	let result = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) };

	zero_result(result, IoOperation::Remove)
}

fn classify_at_error(
	parent: &File,
	name: &CStr,
	error: io::Error,
	expected: ExpectedKind,
) -> PathError {
	let mut status = MaybeUninit::<stat>::uninit();
	// SAFETY: `parent` and `name` are valid for this call; `status` points to enough
	// writable memory and is read only when `fstatat` reports success.
	let result = unsafe {
		libc::fstatat(parent.as_raw_fd(), name.as_ptr(), status.as_mut_ptr(), AT_SYMLINK_NOFOLLOW)
	};

	if result == 0 {
		// SAFETY: successful `fstatat` initialized the complete `stat` value.
		let status = unsafe { status.assume_init() };
		let kind = status.st_mode & S_IFMT;

		if kind == S_IFLNK {
			return PathError::Symlink;
		}
		if expected == ExpectedKind::Directory && kind != S_IFDIR {
			return PathError::UnexpectedDirectoryKind;
		}
		if expected == ExpectedKind::File && kind != S_IFREG {
			return PathError::UnexpectedFileKind;
		}
	}

	paths::io_error(IoOperation::Open, error)
}

fn verify_private_directory_metadata(metadata: &Metadata) -> Result<(), PathError> {
	if !metadata.is_dir() {
		return Err(PathError::UnexpectedDirectoryKind);
	}

	let mode = metadata.mode() & 0o777;

	if metadata.uid() != effective_user_id() || mode != PRIVATE_DIRECTORY_MODE {
		return Err(PathError::InsecurePermissions);
	}

	Ok(())
}

fn verify_private_file_metadata(metadata: &Metadata) -> Result<(), PathError> {
	if !metadata.is_file() {
		return Err(PathError::UnexpectedFileKind);
	}

	let mode = metadata.mode() & 0o777;

	if metadata.uid() != effective_user_id()
		|| mode & 0o077 != 0
		|| mode & 0o400 == 0
		|| mode & 0o111 != 0
	{
		return Err(PathError::InsecurePermissions);
	}

	Ok(())
}

fn verify_private_database_file_metadata(metadata: &Metadata) -> Result<(), PathError> {
	if !metadata.is_file() {
		return Err(PathError::UnexpectedFileKind);
	}

	if metadata.uid() != effective_user_id()
		|| metadata.mode() & 0o777 != PRIVATE_FILE_MODE
		|| metadata.nlink() != 1
	{
		return Err(PathError::InsecurePermissions);
	}

	Ok(())
}

fn effective_user_id() -> uid_t {
	// SAFETY: `geteuid` takes no pointers and has no preconditions.
	unsafe { libc::geteuid() }
}

fn file_from_descriptor(descriptor: RawFd, operation: IoOperation) -> Result<File, PathError> {
	if descriptor == -1 {
		return Err(paths::io_error(operation, io::Error::last_os_error()));
	}

	// SAFETY: every caller passes a newly returned, successful descriptor and transfers
	// sole ownership to this `File`.
	Ok(unsafe { File::from_raw_fd(descriptor) })
}

fn zero_result(result: i32, operation: IoOperation) -> Result<(), PathError> {
	if result == -1 {
		return Err(paths::io_error(operation, io::Error::last_os_error()));
	}

	Ok(())
}

fn c_name(name: &OsStr) -> Result<CString, PathError> {
	CString::new(name.as_bytes()).map_err(|_| PathError::Escape)
}

#[cfg(target_vendor = "apple")]
fn clear_errno() {
	// SAFETY: `__error` returns the calling thread's errno slot.
	unsafe { *libc::__error() = 0 };
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn clear_errno() {
	// SAFETY: `__errno_location` returns the calling thread's errno slot.
	unsafe { *libc::__errno_location() = 0 };
}

#[cfg(not(any(target_vendor = "apple", target_os = "android", target_os = "linux")))]
fn clear_errno() {}

#[cfg(any(target_vendor = "apple", target_os = "android", target_os = "linux"))]
fn current_errno() -> i32 {
	io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

#[cfg(not(any(target_vendor = "apple", target_os = "android", target_os = "linux")))]
fn current_errno() -> i32 {
	0
}

fn hex(bytes: &[u8]) -> String {
	const HEX: &[u8; 16] = b"0123456789abcdef";

	let mut encoded = String::with_capacity(bytes.len() * 2);

	for &byte in bytes {
		encoded.push(char::from(HEX[usize::from(byte >> 4)]));
		encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
	}

	encoded
}
