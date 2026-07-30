//! Descriptor-rooted owner-only filesystem access for disposable Radar cache state.

use std::{
	ffi::{CStr, CString, OsStr, OsString},
	fs::{File, Metadata},
	io::{Read as _, Write as _},
	os::{
		fd::{AsRawFd as _, FromRawFd as _, RawFd},
		unix::{
			ffi::{OsStrExt as _, OsStringExt as _},
			fs::MetadataExt as _,
		},
	},
	path::{Component, Path, PathBuf},
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::prelude::{Result, eyre};

const CACHE_MARKER: [&str; 4] = [".agent", "automations", "radar", "cache"];
const LOCK_FILE_NAME: &str = ".radar.lock";
const MAX_PRIVATE_READ_BYTES: u64 = 64 * 1024 * 1024;
const PRIVATE_DIR_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
pub(crate) const TEMP_FILE_PREFIX: &str = ".radar-tmp-";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PrivateFileIdentity {
	dev: u64,
	ino: u64,
	mtime_seconds: i64,
	mtime_nanoseconds: i64,
	size: u64,
}
impl PrivateFileIdentity {
	pub(crate) fn modified(&self) -> SystemTime {
		if self.mtime_seconds < 0 || self.mtime_nanoseconds < 0 {
			return UNIX_EPOCH;
		}

		UNIX_EPOCH
			+ Duration::from_secs(self.mtime_seconds as u64)
			+ Duration::from_nanos(self.mtime_nanoseconds as u64)
	}

	pub(crate) fn size(&self) -> u64 {
		self.size
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrivateEntryKind {
	Directory,
	File,
}

#[derive(Clone, Debug)]
pub(crate) struct PrivateEntry {
	pub(crate) name: OsString,
	pub(crate) kind: PrivateEntryKind,
	pub(crate) identity: Option<PrivateFileIdentity>,
}

#[derive(Debug)]
pub(crate) struct PrivateCache {
	root_path: PathBuf,
	root: File,
	root_identity: DirectoryIdentity,
	#[cfg(test)]
	direct_root: bool,
}
impl PrivateCache {
	pub(crate) fn root_path(&self) -> &Path {
		&self.root_path
	}

	pub(crate) fn open_or_create(path: &Path) -> Result<Self> {
		open_cache_root(path, true)
	}

	pub(crate) fn open_existing(path: &Path) -> Result<Self> {
		open_cache_root(path, false)
	}

	pub(crate) fn lock(self) -> Result<RadarCacheLock> {
		self.lock_with_flags(false)
	}

	#[cfg(test)]
	pub(crate) fn try_lock(self) -> Result<RadarCacheLock> {
		self.lock_with_flags(true)
	}

	fn lock_with_flags(self, nonblocking: bool) -> Result<RadarCacheLock> {
		self.verify_binding()?;
		let relative = Path::new(LOCK_FILE_NAME);
		let (parent, name) = self.open_parent(relative, true)?;
		let file = open_or_create_regular_file(parent.as_raw_fd(), &name)?;
		let identity = validate_open_file(&file, "lock file")?;
		let operation = libc::LOCK_EX | if nonblocking { libc::LOCK_NB } else { 0 };

		if unsafe { libc::flock(file.as_raw_fd(), operation) } == -1 {
			return Err(std::io::Error::last_os_error().into());
		}

		let current = file_snapshot_at(parent.as_raw_fd(), &name)?
			.ok_or_else(|| eyre::eyre!("Radar cache lock disappeared during acquisition"))?;
		validate_private_file_snapshot(&current, "lock file")?;
		if current.identity != identity {
			eyre::bail!("Radar cache lock identity changed during acquisition");
		}
		self.verify_binding()?;

		Ok(RadarCacheLock { cache: self, file, identity })
	}

	pub(crate) fn read(&self, relative: &Path) -> Result<Vec<u8>> {
		self.read_bounded(relative, MAX_PRIVATE_READ_BYTES)
	}

	pub(crate) fn read_bounded(&self, relative: &Path, max_bytes: u64) -> Result<Vec<u8>> {
		self.read_bounded_with(relative, max_bytes, || {})
	}

	fn read_bounded_with(
		&self,
		relative: &Path,
		max_bytes: u64,
		after_metadata: impl FnOnce(),
	) -> Result<Vec<u8>> {
		let (mut file, initial) = self.open_regular_file(relative)?;

		if initial.size > max_bytes {
			eyre::bail!("Radar cache file exceeds the bounded read limit");
		}
		after_metadata();

		let mut payload = Vec::with_capacity(initial.size as usize);
		let read_limit = max_bytes
			.checked_add(1)
			.ok_or_else(|| eyre::eyre!("Radar cache read limit is too large"))?;

		std::io::Read::by_ref(&mut file).take(read_limit).read_to_end(&mut payload)?;
		if u64::try_from(payload.len()).unwrap_or(u64::MAX) > max_bytes {
			eyre::bail!("Radar cache file exceeds the bounded read limit");
		}

		let final_identity = validate_open_file(&file, "file")?;
		let (parent, name) = self.open_parent(relative, false)?;
		let current = file_snapshot_at(parent.as_raw_fd(), &name)?
			.ok_or_else(|| eyre::eyre!("Radar cache file disappeared during read"))?;
		validate_private_file_snapshot(&current, "file")?;

		if final_identity != initial || current.identity != initial {
			eyre::bail!("Radar cache file identity changed during read");
		}
		self.verify_binding()?;

		Ok(payload)
	}

	pub(crate) fn entries(&self, relative: &Path) -> Result<Vec<PrivateEntry>> {
		let directory = self.open_directory(relative, false)?;
		let mut entries = directory_entries(directory.as_raw_fd())?;

		entries.sort_by(|left, right| left.name.cmp(&right.name));
		self.verify_binding()?;

		Ok(entries)
	}

	pub(crate) fn entries_if_present(&self, relative: &Path) -> Result<Vec<PrivateEntry>> {
		match self.entries(relative) {
			Ok(entries) => Ok(entries),
			Err(error) if is_not_found(&error) => Ok(Vec::new()),
			Err(error) => Err(error),
		}
	}

	pub(crate) fn metadata(&self, relative: &Path) -> Result<Option<PrivateFileIdentity>> {
		let (parent, name) = match self.open_parent(relative, false) {
			Ok(value) => value,
			Err(error) if is_not_found(&error) => return Ok(None),
			Err(error) => return Err(error),
		};
		let snapshot = file_snapshot_at(parent.as_raw_fd(), &name)?;

		snapshot
			.map(|snapshot| {
				validate_private_file_snapshot(&snapshot, "file")?;

				Ok(snapshot.identity)
			})
			.transpose()
	}

	pub(crate) fn create_directory_all(&self, relative: &Path) -> Result<()> {
		drop(self.open_directory(relative, true)?);
		self.verify_binding()
	}

	#[cfg(test)]
	pub(crate) fn create_new_file(&self, relative: &Path) -> Result<File> {
		let (parent, name) = self.open_parent(relative, true)?;
		let file = create_regular_file(parent.as_raw_fd(), &name)?;

		validate_open_file(&file, "file")?;
		parent.sync_all()?;
		self.verify_binding()?;

		Ok(file)
	}

	pub(crate) fn verify_file(
		&self,
		relative: &Path,
		expected: &PrivateFileIdentity,
	) -> Result<()> {
		let (file, identity) = self.open_regular_file(relative)?;

		drop(file);
		if &identity != expected {
			eyre::bail!("Radar cache file identity changed");
		}

		Ok(())
	}

	fn open_regular_file(&self, relative: &Path) -> Result<(File, PrivateFileIdentity)> {
		self.verify_binding()?;
		let (parent, name) = self.open_parent(relative, false)?;
		let file = open_regular_file(parent.as_raw_fd(), &name)?;
		let identity = validate_open_file(&file, "file")?;
		let current = file_snapshot_at(parent.as_raw_fd(), &name)?
			.ok_or_else(|| eyre::eyre!("Radar cache file disappeared during open"))?;

		validate_private_file_snapshot(&current, "file")?;
		if current.identity != identity {
			eyre::bail!("Radar cache file identity changed during open");
		}
		self.verify_binding()?;

		Ok((file, identity))
	}

	fn open_parent(&self, relative: &Path, create: bool) -> Result<(File, CString)> {
		let components = relative_components(relative)?;
		let (name, directories) = components
			.split_last()
			.ok_or_else(|| eyre::eyre!("Radar cache file path must include a file name"))?;
		let directory = self.open_directory_components(directories, create)?;

		Ok((directory, c_string(name)?))
	}

	fn open_directory(&self, relative: &Path, create: bool) -> Result<File> {
		let components = relative_components(relative)?;

		self.open_directory_components(&components, create)
	}

	fn open_directory_components(&self, components: &[OsString], create: bool) -> Result<File> {
		self.verify_binding()?;
		let mut directory = duplicate_file(&self.root)?;

		for component in components {
			let name = c_string(component)?;

			directory = open_or_create_directory(directory.as_raw_fd(), &name, create)?;
			validate_private_directory(&directory, "directory")?;
		}
		self.verify_binding()?;

		Ok(directory)
	}

	fn verify_binding(&self) -> Result<()> {
		#[cfg(test)]
		let reopened = if self.direct_root {
			open_directory_path_direct(&self.root_path)?
		} else {
			open_cache_root_file(&self.root_path, false)?
		};
		#[cfg(not(test))]
		let reopened = open_cache_root_file(&self.root_path, false)?;
		let identity = validate_private_directory(&reopened, "root")?;

		if identity != self.root_identity {
			eyre::bail!("Radar cache root identity changed");
		}

		Ok(())
	}
}

#[derive(Debug)]
pub(crate) struct RadarCacheLock {
	cache: PrivateCache,
	file: File,
	identity: PrivateFileIdentity,
}
impl RadarCacheLock {
	pub(crate) fn cache(&self) -> &PrivateCache {
		&self.cache
	}

	pub(crate) fn read(&self, relative: &Path) -> Result<Vec<u8>> {
		self.read_bounded(relative, MAX_PRIVATE_READ_BYTES)
	}

	pub(crate) fn read_bounded(&self, relative: &Path, max_bytes: u64) -> Result<Vec<u8>> {
		self.verify_lock()?;
		let payload = self.cache.read_bounded(relative, max_bytes)?;

		self.verify_lock()?;

		Ok(payload)
	}

	pub(crate) fn write_atomic(&self, relative: &Path, payload: &[u8]) -> Result<()> {
		validate_write_destination(relative)?;
		self.verify_lock()?;
		let (parent, name) = self.cache.open_parent(relative, true)?;
		let original = file_snapshot_at(parent.as_raw_fd(), &name)?;

		if let Some(snapshot) = &original {
			validate_private_file_snapshot(snapshot, "file")?;
		}

		let temp_name = temporary_name()?;
		let temp = create_regular_file(parent.as_raw_fd(), &temp_name)?;
		let result = write_and_replace(
			&self.cache,
			&parent,
			&name,
			&temp_name,
			temp,
			original.as_ref(),
			payload,
		);

		if result.is_err() {
			let _ = unlink_at(parent.as_raw_fd(), &temp_name);
		}

		result?;
		self.verify_lock()
	}

	pub(crate) fn write_atomic_if_matches(
		&self,
		relative: &Path,
		expected: Option<&PrivateFileIdentity>,
		payload: &[u8],
	) -> Result<PrivateFileIdentity> {
		validate_write_destination(relative)?;
		self.verify_lock()?;
		let (parent, name) = self.cache.open_parent(relative, true)?;
		let original = file_snapshot_at(parent.as_raw_fd(), &name)?;

		if !same_optional_identity(expected, original.as_ref()) {
			eyre::bail!("Radar cache destination identity changed before atomic replacement");
		}
		if let Some(snapshot) = &original {
			validate_private_file_snapshot(snapshot, "file")?;
		}

		let temp_name = temporary_name()?;
		let temp = create_regular_file(parent.as_raw_fd(), &temp_name)?;
		let result = write_and_replace(
			&self.cache,
			&parent,
			&name,
			&temp_name,
			temp,
			original.as_ref(),
			payload,
		);

		if result.is_err() {
			let _ = unlink_at(parent.as_raw_fd(), &temp_name);
		}

		let identity = result?;

		self.verify_lock()?;

		Ok(identity)
	}

	pub(crate) fn remove_if_matches(
		&self,
		relative: &Path,
		expected: &PrivateFileIdentity,
	) -> Result<()> {
		self.verify_lock()?;
		let (parent, name) = self.cache.open_parent(relative, false)?;
		let current = file_snapshot_at(parent.as_raw_fd(), &name)?
			.ok_or_else(|| eyre::eyre!("Radar cache file disappeared before retention"))?;

		validate_private_file_snapshot(&current, "file")?;
		if &current.identity != expected {
			eyre::bail!("Radar cache file identity changed before retention");
		}

		let file = open_regular_file(parent.as_raw_fd(), &name)?;
		let opened = validate_open_file(&file, "file")?;

		if &opened != expected {
			eyre::bail!("Radar cache file identity changed before unlink");
		}

		let revalidated = file_snapshot_at(parent.as_raw_fd(), &name)?
			.ok_or_else(|| eyre::eyre!("Radar cache file disappeared before unlink"))?;
		if revalidated.identity != *expected {
			eyre::bail!("Radar cache file identity changed before unlink");
		}

		unlink_at(parent.as_raw_fd(), &name)?;
		parent.sync_all()?;
		self.verify_lock()
	}

	pub(crate) fn create_directory_atomic(
		&self,
		relative: &Path,
		files: &[(&str, &[u8])],
	) -> Result<bool> {
		let components = relative_components(relative)?;
		let (name, directories) = components
			.split_last()
			.ok_or_else(|| eyre::eyre!("Radar cache directory path must include a name"))?;
		let parent = self.cache.open_directory_components(directories, true)?;
		let name = c_string(name)?;

		self.verify_lock()?;
		if let Some(snapshot) = file_snapshot_at(parent.as_raw_fd(), &name)? {
			if snapshot.file_type != u32::from(libc::S_IFDIR) {
				eyre::bail!("Radar committed pair destination is not a directory");
			}
			let directory = open_directory_at(parent.as_raw_fd(), &name)?;

			validate_private_directory(&directory, "committed pair directory")?;

			return Ok(false);
		}

		let temp_name = temporary_name()?;
		if unsafe {
			libc::mkdirat(parent.as_raw_fd(), temp_name.as_ptr(), PRIVATE_DIR_MODE as libc::mode_t)
		} == -1
		{
			return Err(std::io::Error::last_os_error().into());
		}
		let result = (|| -> Result<()> {
			let directory = open_directory_at(parent.as_raw_fd(), &temp_name)?;

			validate_private_directory(&directory, "temporary committed pair directory")?;
			for (file_name, payload) in files {
				validate_leaf_name(file_name)?;
				let file_name = CString::new(*file_name)
					.map_err(|_| eyre::eyre!("Radar pair file name contains NUL"))?;
				let mut file = create_regular_file(directory.as_raw_fd(), &file_name)?;

				file.write_all(payload)?;
				file.sync_all()?;
				validate_open_file(&file, "committed pair file")?;
			}
			directory.sync_all()?;
			if file_snapshot_at(parent.as_raw_fd(), &name)?.is_some() {
				eyre::bail!("Radar committed pair destination appeared before commit");
			}
			if unsafe {
				libc::renameat(
					parent.as_raw_fd(),
					temp_name.as_ptr(),
					parent.as_raw_fd(),
					name.as_ptr(),
				)
			} == -1
			{
				return Err(std::io::Error::last_os_error().into());
			}
			parent.sync_all()?;
			self.cache.verify_binding()
		})();

		if result.is_err() {
			let _ = remove_directory_tree_at(parent.as_raw_fd(), &temp_name);
		}
		result?;
		self.verify_lock()?;

		Ok(true)
	}

	pub(crate) fn remove_directory_atomic(&self, relative: &Path) -> Result<()> {
		let components = relative_components(relative)?;
		let (name, directories) = components
			.split_last()
			.ok_or_else(|| eyre::eyre!("Radar cache directory path must include a name"))?;
		let parent = self.cache.open_directory_components(directories, false)?;
		let name = c_string(name)?;
		let directory = open_directory_at(parent.as_raw_fd(), &name)?;

		validate_private_directory(&directory, "directory removal target")?;
		drop(directory);
		let temp_name = temporary_name()?;

		self.verify_lock()?;
		if unsafe {
			libc::renameat(
				parent.as_raw_fd(),
				name.as_ptr(),
				parent.as_raw_fd(),
				temp_name.as_ptr(),
			)
		} == -1
		{
			return Err(std::io::Error::last_os_error().into());
		}
		parent.sync_all()?;
		remove_directory_tree_at(parent.as_raw_fd(), &temp_name)?;
		parent.sync_all()?;
		self.verify_lock()
	}

	pub(crate) fn bootstrap_cache_is_empty(&self) -> Result<bool> {
		self.verify_lock()?;
		let entries = self.cache.entries(Path::new(""))?;

		Ok(entries.iter().all(|entry| entry.name == OsStr::new(LOCK_FILE_NAME)))
	}

	pub(crate) fn relative_path(&self, path: &Path) -> Result<PathBuf> {
		let location = private_file_path(path)?;
		let expected_root = absolute_path_without_traversal(&self.cache.root_path)?;
		let actual_root = absolute_path_without_traversal(&location.root)?;

		if actual_root != expected_root {
			eyre::bail!("Radar cache path must share the ledger cache lock root");
		}

		Ok(location.relative)
	}

	fn verify_lock(&self) -> Result<()> {
		let identity = validate_open_file(&self.file, "lock file")?;

		if identity != self.identity {
			eyre::bail!("Radar cache lock identity changed");
		}
		self.cache.verify_file(Path::new(LOCK_FILE_NAME), &self.identity)
	}
}
impl Drop for RadarCacheLock {
	fn drop(&mut self) {
		unsafe {
			libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectoryIdentity {
	dev: u64,
	ino: u64,
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct PrivateTestDirectory {
	parent_path: PathBuf,
	parent: File,
	parent_identity: DirectoryIdentity,
	name: CString,
	path: PathBuf,
	directory: File,
	identity: DirectoryIdentity,
}
#[cfg(test)]
impl PrivateTestDirectory {
	pub(crate) fn path(&self) -> &Path {
		&self.path
	}

	fn remove(&self) -> Result<()> {
		self.remove_with_before_unlink(|| {})
	}

	pub(crate) fn remove_with_before_unlink(&self, before_unlink: impl FnOnce()) -> Result<()> {
		verify_test_parent_binding(&self.parent_path, &self.parent, &self.parent_identity)?;
		let identity = directory_identity(&self.directory, "test directory")?;

		if identity != self.identity {
			eyre::bail!("Radar test directory identity changed before cleanup");
		}
		remove_test_directory_contents(&self.directory)?;
		before_unlink();
		verify_directory_binding_at(
			self.parent.as_raw_fd(),
			&self.name,
			&self.identity,
			"test directory",
		)?;
		if unsafe {
			libc::unlinkat(self.parent.as_raw_fd(), self.name.as_ptr(), libc::AT_REMOVEDIR)
		} == -1
		{
			return Err(std::io::Error::last_os_error().into());
		}
		self.parent.sync_all()?;
		verify_test_parent_binding(&self.parent_path, &self.parent, &self.parent_identity)
	}
}
#[cfg(test)]
impl Drop for PrivateTestDirectory {
	fn drop(&mut self) {
		if let Err(error) = self.remove()
			&& !std::thread::panicking()
		{
			panic!("private Radar test directory cleanup failed: {error}");
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileSnapshot {
	identity: PrivateFileIdentity,
	mode: u32,
	uid: u32,
	nlink: u64,
	file_type: u32,
}

fn open_cache_root(path: &Path, create: bool) -> Result<PrivateCache> {
	let root_path = absolute_path_without_traversal(path)?;

	#[cfg(test)]
	if std::env::var_os("DECODEX_CANDIDATE_SANDBOX").is_some_and(|value| value == OsStr::new("1"))
		&& let Some(cache) = open_sandbox_cache_root(&root_path, create)?
	{
		return Ok(cache);
	}

	let root = open_cache_root_file(&root_path, create)?;
	let root_identity = validate_private_directory(&root, "root")?;

	Ok(PrivateCache {
		root_path,
		root,
		root_identity,
		#[cfg(test)]
		direct_root: false,
	})
}

#[cfg(test)]
fn open_sandbox_cache_root(path: &Path, create: bool) -> Result<Option<PrivateCache>> {
	let sandbox_root = std::env::var_os("TMPDIR")
		.ok_or_else(|| eyre::eyre!("sandboxed Radar tests require TMPDIR"))?;
	let sandbox_root = absolute_path_without_traversal(Path::new(&sandbox_root))?;

	if let Ok(relative) = path.strip_prefix(&sandbox_root) {
		return open_sandbox_private_cache_root(path, &sandbox_root, relative, create).map(Some);
	}

	let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
	let candidate = manifest
		.parent()
		.and_then(Path::parent)
		.ok_or_else(|| eyre::eyre!("Radar manifest must be inside the candidate repository"))?;

	if let Ok(relative) = path.strip_prefix(candidate) {
		if create {
			eyre::bail!(
				"sandboxed Radar tests cannot create state inside the candidate repository"
			);
		}

		return open_sandbox_candidate_cache_root(path, candidate, relative).map(Some);
	}

	Ok(None)
}

#[cfg(test)]
fn open_sandbox_private_cache_root(
	path: &Path,
	sandbox_root: &Path,
	relative: &Path,
	create: bool,
) -> Result<PrivateCache> {
	let metadata = std::fs::symlink_metadata(sandbox_root)?;

	if metadata.file_type().is_symlink() || !metadata.is_dir() {
		eyre::bail!("sandboxed Radar test root must be a non-symlink directory");
	}
	validate_owner_mode_link(
		metadata.uid(),
		metadata.mode() & 0o777,
		metadata.nlink(),
		PRIVATE_DIR_MODE,
		"sandboxed test root",
		false,
	)?;

	let mut directory = open_directory_path_direct(sandbox_root)?;
	let sandbox_identity = validate_private_directory(&directory, "sandboxed test root")?;

	if sandbox_identity != (DirectoryIdentity { dev: metadata.dev(), ino: metadata.ino() }) {
		eyre::bail!("sandboxed Radar test root identity changed during open");
	}
	for component in relative_components(relative)? {
		let name = c_string(&component)?;

		directory = open_or_create_directory(directory.as_raw_fd(), &name, create)?;
		validate_private_directory(&directory, "sandboxed test directory")?;
	}

	let root_identity = validate_private_directory(&directory, "root")?;

	Ok(PrivateCache {
		root_path: path.to_path_buf(),
		root: directory,
		root_identity,
		direct_root: true,
	})
}

#[cfg(test)]
fn open_sandbox_candidate_cache_root(
	path: &Path,
	candidate: &Path,
	relative: &Path,
) -> Result<PrivateCache> {
	let metadata = std::fs::symlink_metadata(candidate)?;

	if metadata.file_type().is_symlink() || !metadata.is_dir() {
		eyre::bail!("sandboxed Radar candidate root must be a non-symlink directory");
	}

	let mut directory = open_directory_path_direct(candidate)?;
	let opened = directory.metadata()?;

	if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
		eyre::bail!("sandboxed Radar candidate root identity changed during open");
	}

	let (_, candidate_components) = absolute_components(candidate)?;
	let (_, path_components) = absolute_components(path)?;
	let private_start = cache_private_start(&path_components);

	for (offset, component) in relative_components(relative)?.into_iter().enumerate() {
		let name = c_string(&component)?;

		directory = open_or_create_directory(directory.as_raw_fd(), &name, false)?;
		if candidate_components.len() + offset >= private_start {
			validate_private_directory(&directory, "sandboxed candidate cache directory")?;
		}
	}

	let root_identity = validate_private_directory(&directory, "root")?;

	Ok(PrivateCache {
		root_path: path.to_path_buf(),
		root: directory,
		root_identity,
		direct_root: true,
	})
}

fn open_cache_root_file(path: &Path, create: bool) -> Result<File> {
	let (_, components) = absolute_components(path)?;
	let private_start = cache_private_start(&components);
	let mut directory = File::open("/")?;

	for (index, component) in components.iter().enumerate() {
		let name = c_string(component)?;

		directory = open_or_create_directory(directory.as_raw_fd(), &name, create)?;
		if index >= private_start {
			validate_private_directory(&directory, "directory")?;
		}
	}

	Ok(directory)
}

fn cache_private_start(components: &[OsString]) -> usize {
	components
		.windows(CACHE_MARKER.len())
		.position(|window| {
			window.iter().zip(CACHE_MARKER).all(|(actual, expected)| actual == OsStr::new(expected))
		})
		.map_or_else(|| components.len().saturating_sub(1), |index| index + CACHE_MARKER.len() - 1)
}

fn absolute_path_without_traversal(path: &Path) -> Result<PathBuf> {
	let (absolute, _) = absolute_components(path)?;

	Ok(absolute)
}

fn absolute_components(path: &Path) -> Result<(PathBuf, Vec<OsString>)> {
	reject_unsafe_components(path)?;
	let absolute =
		if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir()?.join(path) };
	let mut components = Vec::new();

	for component in absolute.components() {
		match component {
			Component::RootDir => {},
			Component::Normal(value) => components.push(value.to_os_string()),
			Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
				eyre::bail!("Radar cache path contains an unsupported traversal component");
			},
		}
	}
	if components.is_empty() {
		eyre::bail!("Radar cache root cannot be the filesystem root");
	}

	Ok((absolute, components))
}

fn reject_unsafe_components(path: &Path) -> Result<()> {
	if path.components().any(|component| matches!(component, Component::ParentDir)) {
		eyre::bail!("Radar cache path must not contain '..'");
	}

	Ok(())
}

fn relative_components(path: &Path) -> Result<Vec<OsString>> {
	if path.is_absolute() {
		eyre::bail!("Radar cache-relative path must not be absolute");
	}

	let mut components = Vec::new();

	for component in path.components() {
		match component {
			Component::Normal(value) => components.push(value.to_os_string()),
			Component::CurDir => {},
			Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
				eyre::bail!("Radar cache-relative path contains an unsafe component");
			},
		}
	}

	Ok(components)
}

fn validate_write_destination(path: &Path) -> Result<()> {
	let components = relative_components(path)?;
	let name = components
		.last()
		.ok_or_else(|| eyre::eyre!("Radar cache output path must include a file name"))?;

	if name == OsStr::new(LOCK_FILE_NAME)
		|| name.as_bytes().starts_with(TEMP_FILE_PREFIX.as_bytes())
	{
		eyre::bail!("Radar cache output path uses a reserved internal file name");
	}

	Ok(())
}

fn validate_leaf_name(name: &str) -> Result<()> {
	if name.is_empty()
		|| name == "."
		|| name == ".."
		|| name.contains('/')
		|| name.as_bytes().starts_with(TEMP_FILE_PREFIX.as_bytes())
	{
		eyre::bail!("Radar pair file name is unsafe");
	}

	Ok(())
}

fn open_or_create_directory(parent: RawFd, name: &CStr, create: bool) -> Result<File> {
	match open_directory_at(parent, name) {
		Ok(file) => Ok(file),
		Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
			if unsafe { libc::mkdirat(parent, name.as_ptr(), PRIVATE_DIR_MODE as libc::mode_t) }
				== -1
			{
				let mkdir_error = std::io::Error::last_os_error();

				if mkdir_error.kind() != std::io::ErrorKind::AlreadyExists {
					return Err(mkdir_error.into());
				}
			}

			open_directory_at(parent, name).map_err(Into::into)
		},
		Err(error) => Err(error.into()),
	}
}

fn open_directory_at(parent: RawFd, name: &CStr) -> std::io::Result<File> {
	let fd = unsafe {
		libc::openat(
			parent,
			name.as_ptr(),
			libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
		)
	};

	if fd == -1 {
		Err(std::io::Error::last_os_error())
	} else {
		Ok(unsafe { File::from_raw_fd(fd) })
	}
}

#[cfg(test)]
fn open_directory_path_direct(path: &Path) -> Result<File> {
	let path = CString::new(path.as_os_str().as_bytes())
		.map_err(|_| eyre::eyre!("Radar test root path contains NUL"))?;
	let fd = unsafe {
		libc::open(
			path.as_ptr(),
			libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
		)
	};

	if fd == -1 {
		Err(std::io::Error::last_os_error().into())
	} else {
		Ok(unsafe { File::from_raw_fd(fd) })
	}
}

#[cfg(test)]
fn open_test_parent_path(path: &Path) -> Result<File> {
	if std::env::var_os("DECODEX_CANDIDATE_SANDBOX").as_deref() == Some(OsStr::new("1")) {
		let sandbox_root = std::env::var_os("TMPDIR")
			.ok_or_else(|| eyre::eyre!("sandboxed Radar tests require TMPDIR"))?;
		let sandbox_root = absolute_path_without_traversal(Path::new(&sandbox_root))?;
		let relative = path
			.strip_prefix(&sandbox_root)
			.map_err(|_| eyre::eyre!("sandboxed Radar test parent must stay under TMPDIR"))?;

		return Ok(open_sandbox_private_cache_root(path, &sandbox_root, relative, false)?.root);
	}

	let (_, components) = absolute_components(path)?;
	let mut directory = File::open("/")?;

	for component in components {
		directory = open_directory_at(directory.as_raw_fd(), &c_string(&component)?)?;
	}

	Ok(directory)
}

#[cfg(test)]
fn validate_test_parent_directory(directory: &File) -> Result<DirectoryIdentity> {
	let metadata = directory.metadata()?;

	if !metadata.is_dir() {
		eyre::bail!("Radar test parent must be a directory");
	}

	let permissions = metadata.mode() & 0o777;
	let private_parent =
		metadata.uid() == unsafe { libc::geteuid() } && permissions == PRIVATE_DIR_MODE;
	let system_temporary_parent =
		metadata.uid() == 0 && permissions == 0o777 && metadata.mode() & 0o1000 == 0o1000;

	if !private_parent && !system_temporary_parent {
		eyre::bail!(
			"Radar test parent must be owner-private or a root-owned sticky temporary directory"
		);
	}

	Ok(DirectoryIdentity { dev: metadata.dev(), ino: metadata.ino() })
}

#[cfg(test)]
fn verify_test_parent_binding(
	path: &Path,
	held: &File,
	expected: &DirectoryIdentity,
) -> Result<()> {
	let held_identity = validate_test_parent_directory(held)?;
	let reopened = open_test_parent_path(path)?;
	let reopened_identity = validate_test_parent_directory(&reopened)?;

	if &held_identity != expected || &reopened_identity != expected {
		eyre::bail!("Radar test parent identity changed");
	}

	Ok(())
}

fn open_regular_file(parent: RawFd, name: &CStr) -> Result<File> {
	let fd = unsafe {
		libc::openat(parent, name.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
	};

	if fd == -1 {
		Err(std::io::Error::last_os_error().into())
	} else {
		Ok(unsafe { File::from_raw_fd(fd) })
	}
}

fn create_regular_file(parent: RawFd, name: &CStr) -> Result<File> {
	let fd = unsafe {
		libc::openat(
			parent,
			name.as_ptr(),
			libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
			PRIVATE_FILE_MODE as libc::c_uint,
		)
	};

	if fd == -1 {
		Err(std::io::Error::last_os_error().into())
	} else {
		Ok(unsafe { File::from_raw_fd(fd) })
	}
}

fn open_or_create_regular_file(parent: RawFd, name: &CStr) -> Result<File> {
	match open_regular_file(parent, name) {
		Ok(file) => Ok(file),
		Err(error)
			if error
				.downcast_ref::<std::io::Error>()
				.is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
			match create_regular_file(parent, name) {
				Ok(file) => Ok(file),
				Err(create_error)
					if create_error
						.downcast_ref::<std::io::Error>()
						.is_some_and(|error| error.kind() == std::io::ErrorKind::AlreadyExists) =>
					open_regular_file(parent, name),
				Err(create_error) => Err(create_error),
			},
		Err(error) => Err(error),
	}
}

fn write_and_replace(
	cache: &PrivateCache,
	parent: &File,
	name: &CStr,
	temp_name: &CStr,
	mut temp: File,
	original: Option<&FileSnapshot>,
	payload: &[u8],
) -> Result<PrivateFileIdentity> {
	temp.write_all(payload)?;
	temp.sync_all()?;
	let temp_identity = validate_open_file(&temp, "temporary file")?;
	let current = file_snapshot_at(parent.as_raw_fd(), name)?;

	if !same_optional_snapshot(original, current.as_ref()) {
		eyre::bail!("Radar cache destination identity changed before atomic replacement");
	}
	if let Some(snapshot) = &current {
		validate_private_file_snapshot(snapshot, "file")?;
	}

	if unsafe {
		libc::renameat(parent.as_raw_fd(), temp_name.as_ptr(), parent.as_raw_fd(), name.as_ptr())
	} == -1
	{
		return Err(std::io::Error::last_os_error().into());
	}

	let installed = file_snapshot_at(parent.as_raw_fd(), name)?
		.ok_or_else(|| eyre::eyre!("Radar cache replacement was not installed"))?;
	validate_private_file_snapshot(&installed, "file")?;
	if installed.identity != temp_identity {
		eyre::bail!("Radar cache replacement identity does not match the written file");
	}
	parent.sync_all()?;
	cache.verify_binding()?;

	Ok(installed.identity)
}

fn same_optional_snapshot(left: Option<&FileSnapshot>, right: Option<&FileSnapshot>) -> bool {
	match (left, right) {
		(Some(left), Some(right)) => left.identity == right.identity,
		(None, None) => true,
		_ => false,
	}
}

fn same_optional_identity(
	left: Option<&PrivateFileIdentity>,
	right: Option<&FileSnapshot>,
) -> bool {
	match (left, right) {
		(Some(left), Some(right)) => *left == right.identity,
		(None, None) => true,
		_ => false,
	}
}

fn file_snapshot_at(parent: RawFd, name: &CStr) -> Result<Option<FileSnapshot>> {
	let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
	let result = unsafe {
		libc::fstatat(parent, name.as_ptr(), stat.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW)
	};

	if result == -1 {
		let error = std::io::Error::last_os_error();

		return if error.kind() == std::io::ErrorKind::NotFound {
			Ok(None)
		} else {
			Err(error.into())
		};
	}

	let stat = unsafe { stat.assume_init() };

	Ok(Some(snapshot_from_stat(&stat)))
}

fn snapshot_from_stat(stat: &libc::stat) -> FileSnapshot {
	FileSnapshot {
		identity: PrivateFileIdentity {
			dev: stat.st_dev as u64,
			ino: stat.st_ino,
			mtime_seconds: stat.st_mtime,
			mtime_nanoseconds: stat_mtime_nanoseconds(stat),
			size: u64::try_from(stat.st_size).unwrap_or_default(),
		},
		mode: u32::from(stat.st_mode & 0o777),
		uid: stat.st_uid,
		nlink: u64::from(stat.st_nlink),
		file_type: u32::from(stat.st_mode & libc::S_IFMT),
	}
}

fn stat_mtime_nanoseconds(stat: &libc::stat) -> i64 {
	stat.st_mtime_nsec
}

fn validate_open_file(file: &File, label: &str) -> Result<PrivateFileIdentity> {
	let metadata = file.metadata()?;

	validate_private_file_metadata(&metadata, label)?;

	Ok(identity_from_metadata(&metadata))
}

fn validate_private_file_metadata(metadata: &Metadata, label: &str) -> Result<()> {
	if !metadata.is_file() {
		eyre::bail!("Radar cache {label} must be a regular file");
	}
	validate_owner_mode_link(
		metadata.uid(),
		metadata.mode() & 0o777,
		metadata.nlink(),
		PRIVATE_FILE_MODE,
		label,
		true,
	)
}

fn validate_private_file_snapshot(snapshot: &FileSnapshot, label: &str) -> Result<()> {
	if snapshot.file_type != u32::from(libc::S_IFREG) {
		eyre::bail!("Radar cache {label} must be a regular non-symlink");
	}
	validate_owner_mode_link(
		snapshot.uid,
		snapshot.mode,
		snapshot.nlink,
		PRIVATE_FILE_MODE,
		label,
		true,
	)
}

fn validate_private_directory(file: &File, label: &str) -> Result<DirectoryIdentity> {
	let metadata = file.metadata()?;

	if !metadata.is_dir() {
		eyre::bail!("Radar cache {label} must be a non-symlink directory");
	}
	validate_owner_mode_link(
		metadata.uid(),
		metadata.mode() & 0o777,
		metadata.nlink(),
		PRIVATE_DIR_MODE,
		label,
		false,
	)?;

	Ok(DirectoryIdentity { dev: metadata.dev(), ino: metadata.ino() })
}

fn validate_owner_mode_link(
	actual_uid: u32,
	actual_mode: u32,
	actual_nlink: u64,
	expected_mode: u32,
	label: &str,
	require_single_link: bool,
) -> Result<()> {
	let expected_uid = unsafe { libc::geteuid() };

	if actual_uid != expected_uid {
		eyre::bail!(
			"Radar cache {label} has the wrong owner (expected uid {expected_uid}, found \
			 {actual_uid})"
		);
	}
	if actual_mode != expected_mode {
		eyre::bail!("Radar cache {label} has mode {actual_mode:04o}; expected {expected_mode:04o}");
	}
	if actual_nlink == 0 || (require_single_link && actual_nlink != 1) {
		eyre::bail!("Radar cache {label} has an invalid link count");
	}

	Ok(())
}

fn identity_from_metadata(metadata: &Metadata) -> PrivateFileIdentity {
	PrivateFileIdentity {
		dev: metadata.dev(),
		ino: metadata.ino(),
		mtime_seconds: metadata.mtime(),
		mtime_nanoseconds: metadata.mtime_nsec(),
		size: metadata.size(),
	}
}

fn directory_entries(fd: RawFd) -> Result<Vec<PrivateEntry>> {
	let duplicate = duplicate_fd(fd)?;
	let stream = unsafe { libc::fdopendir(duplicate) };

	if stream.is_null() {
		let error = std::io::Error::last_os_error();

		unsafe {
			libc::close(duplicate);
		}

		return Err(error.into());
	}

	let stream = DirectoryStream(stream);
	let mut entries = Vec::new();

	loop {
		let entry = unsafe { libc::readdir(stream.0) };

		if entry.is_null() {
			break;
		}

		let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
		if name.to_bytes() == b"." || name.to_bytes() == b".." {
			continue;
		}

		let snapshot = file_snapshot_at(fd, name)?
			.ok_or_else(|| eyre::eyre!("Radar cache entry changed during directory scan"))?;
		let kind = if snapshot.file_type == u32::from(libc::S_IFREG) {
			validate_private_file_snapshot(&snapshot, "file")?;
			PrivateEntryKind::File
		} else if snapshot.file_type == u32::from(libc::S_IFDIR) {
			let child = open_directory_at(fd, name)?;

			validate_private_directory(&child, "directory")?;
			PrivateEntryKind::Directory
		} else {
			eyre::bail!("Radar cache contains a symlink or unsupported entry");
		};

		entries.push(PrivateEntry {
			name: OsString::from_vec(name.to_bytes().to_vec()),
			kind,
			identity: (kind == PrivateEntryKind::File).then_some(snapshot.identity),
		});
	}

	Ok(entries)
}

struct DirectoryStream(*mut libc::DIR);
impl Drop for DirectoryStream {
	fn drop(&mut self) {
		unsafe {
			libc::closedir(self.0);
		}
	}
}

fn duplicate_file(file: &File) -> Result<File> {
	let fd = duplicate_fd(file.as_raw_fd())?;

	Ok(unsafe { File::from_raw_fd(fd) })
}

fn duplicate_fd(fd: RawFd) -> Result<RawFd> {
	let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };

	if duplicate == -1 { Err(std::io::Error::last_os_error().into()) } else { Ok(duplicate) }
}

fn temporary_name() -> Result<CString> {
	let mut nonce = [0_u8; 16];

	getrandom::fill(&mut nonce)
		.map_err(|error| eyre::eyre!("failed to create a Radar temporary-file nonce: {error}"))?;
	let mut name = String::with_capacity(TEMP_FILE_PREFIX.len() + nonce.len() * 2);

	name.push_str(TEMP_FILE_PREFIX);
	for byte in nonce {
		use std::fmt::Write as _;

		write!(&mut name, "{byte:02x}").expect("writing into a String must not fail");
	}

	CString::new(name)
		.map_err(|_| eyre::eyre!("generated Radar temporary name unexpectedly contains NUL"))
}

#[cfg(test)]
fn test_temporary_name() -> Result<CString> {
	let mut nonce = [0_u8; 8];

	getrandom::fill(&mut nonce)
		.map_err(|error| eyre::eyre!("failed to create a Radar test-directory nonce: {error}"))?;
	let mut name = String::with_capacity(3 + nonce.len() * 2);

	name.push_str("rt-");
	for byte in nonce {
		use std::fmt::Write as _;

		write!(&mut name, "{byte:02x}").expect("writing into a String must not fail");
	}

	CString::new(name)
		.map_err(|_| eyre::eyre!("generated Radar test name unexpectedly contains NUL"))
}

fn unlink_at(parent: RawFd, name: &CStr) -> Result<()> {
	if unsafe { libc::unlinkat(parent, name.as_ptr(), 0) } == -1 {
		Err(std::io::Error::last_os_error().into())
	} else {
		Ok(())
	}
}

fn remove_directory_tree_at(parent: RawFd, name: &CStr) -> Result<()> {
	let directory = open_directory_at(parent, name)?;

	for entry in directory_entries(directory.as_raw_fd())? {
		let child_name = c_string(&entry.name)?;

		match entry.kind {
			PrivateEntryKind::Directory =>
				remove_directory_tree_at(directory.as_raw_fd(), &child_name)?,
			PrivateEntryKind::File => unlink_at(directory.as_raw_fd(), &child_name)?,
		}
	}
	drop(directory);
	if unsafe { libc::unlinkat(parent, name.as_ptr(), libc::AT_REMOVEDIR) } == -1 {
		return Err(std::io::Error::last_os_error().into());
	}

	Ok(())
}

#[cfg(test)]
fn remove_test_directory_contents(directory: &File) -> Result<()> {
	let duplicate = duplicate_fd(directory.as_raw_fd())?;
	let stream = unsafe { libc::fdopendir(duplicate) };

	if stream.is_null() {
		let error = std::io::Error::last_os_error();

		unsafe {
			libc::close(duplicate);
		}

		return Err(error.into());
	}

	let stream = DirectoryStream(stream);
	let mut entries = Vec::new();
	loop {
		let entry = unsafe { libc::readdir(stream.0) };

		if entry.is_null() {
			break;
		}

		let child_name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
		if child_name.to_bytes() == b"." || child_name.to_bytes() == b".." {
			continue;
		}
		let snapshot = file_snapshot_at(directory.as_raw_fd(), child_name)?
			.ok_or_else(|| eyre::eyre!("Radar test entry changed during cleanup"))?;

		entries.push((child_name.to_owned(), snapshot));
	}
	drop(stream);

	for (child_name, expected) in entries {
		let current = file_snapshot_at(directory.as_raw_fd(), &child_name)?
			.ok_or_else(|| eyre::eyre!("Radar test entry disappeared during cleanup"))?;

		if current != expected {
			eyre::bail!("Radar test entry identity changed during cleanup");
		}

		if expected.file_type == u32::from(libc::S_IFDIR) {
			let child = open_directory_at(directory.as_raw_fd(), &child_name)?;
			let identity = directory_identity(&child, "test child directory")?;
			let expected_identity =
				DirectoryIdentity { dev: expected.identity.dev, ino: expected.identity.ino };

			if identity != expected_identity {
				eyre::bail!("Radar test child directory identity changed during cleanup");
			}
			remove_test_directory_contents(&child)?;
			verify_directory_binding_at(
				directory.as_raw_fd(),
				&child_name,
				&expected_identity,
				"test child directory",
			)?;
			if unsafe {
				libc::unlinkat(directory.as_raw_fd(), child_name.as_ptr(), libc::AT_REMOVEDIR)
			} == -1
			{
				return Err(std::io::Error::last_os_error().into());
			}
		} else {
			let rebound = file_snapshot_at(directory.as_raw_fd(), &child_name)?
				.ok_or_else(|| eyre::eyre!("Radar test entry disappeared before cleanup"))?;

			if rebound != expected {
				eyre::bail!("Radar test entry identity changed before cleanup");
			}
			unlink_at(directory.as_raw_fd(), &child_name)?;
		}
	}

	Ok(())
}

#[cfg(test)]
fn verify_directory_binding_at(
	parent: RawFd,
	name: &CStr,
	expected: &DirectoryIdentity,
	label: &str,
) -> Result<()> {
	let snapshot = file_snapshot_at(parent, name)?
		.ok_or_else(|| eyre::eyre!("Radar {label} disappeared during cleanup"))?;
	let current = DirectoryIdentity { dev: snapshot.identity.dev, ino: snapshot.identity.ino };

	if snapshot.file_type != u32::from(libc::S_IFDIR) || &current != expected {
		eyre::bail!("Radar {label} identity changed during cleanup");
	}

	Ok(())
}

#[cfg(test)]
fn directory_identity(directory: &File, label: &str) -> Result<DirectoryIdentity> {
	let metadata = directory.metadata()?;

	if !metadata.is_dir() {
		eyre::bail!("Radar {label} must remain a directory");
	}
	if metadata.uid() != unsafe { libc::geteuid() } {
		eyre::bail!("Radar {label} must remain owned by the current user");
	}

	Ok(DirectoryIdentity { dev: metadata.dev(), ino: metadata.ino() })
}

fn c_string(value: &OsStr) -> Result<CString> {
	CString::new(value.as_bytes())
		.map_err(|_| eyre::eyre!("Radar cache path component contains NUL"))
}

fn is_not_found(error: &eyre::Report) -> bool {
	error
		.chain()
		.find_map(|cause| cause.downcast_ref::<std::io::Error>())
		.is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
}

#[derive(Debug)]
struct PrivatePath {
	root: PathBuf,
	relative: PathBuf,
}

fn private_file_path(path: &Path) -> Result<PrivatePath> {
	reject_unsafe_components(path)?;
	let components = path.components().collect::<Vec<_>>();

	if let Some(index) = components.windows(CACHE_MARKER.len()).position(|window| {
		window.iter().zip(CACHE_MARKER).all(|(actual, expected)| {
			matches!(actual, Component::Normal(value) if *value == OsStr::new(expected))
		})
	}) {
		let marker_end = index + CACHE_MARKER.len();
		let root = components[..marker_end].iter().fold(PathBuf::new(), |mut path, component| {
			path.push(component.as_os_str());
			path
		});
		let relative =
			components[marker_end..].iter().fold(PathBuf::new(), |mut path, component| {
				path.push(component.as_os_str());
				path
			});

		if relative.as_os_str().is_empty() {
			eyre::bail!("Radar cache file path must be below the cache root");
		}

		return Ok(PrivatePath { root, relative });
	}

	let root = path
		.parent()
		.filter(|parent| !parent.as_os_str().is_empty())
		.unwrap_or_else(|| Path::new("."))
		.to_path_buf();
	let relative = path
		.file_name()
		.map(PathBuf::from)
		.ok_or_else(|| eyre::eyre!("Radar cache file path must include a file name"))?;

	Ok(PrivatePath { root, relative })
}

pub(crate) fn private_cache_file(path: &Path) -> Result<(PrivateCache, PathBuf)> {
	let location = private_file_path(path)?;
	let cache = PrivateCache::open_or_create(&location.root)?;

	Ok((cache, location.relative))
}

pub(crate) fn read_private_file(path: &Path) -> Result<Vec<u8>> {
	let location = private_file_path(path)?;
	let cache = PrivateCache::open_existing(&location.root)?;

	cache.read(&location.relative)
}

pub(crate) fn read_private_file_under_lock(lock: &RadarCacheLock, path: &Path) -> Result<Vec<u8>> {
	let relative = lock.relative_path(path)?;

	lock.read(&relative)
}

pub(crate) fn read_private_files(paths: &[&Path]) -> Result<Vec<Vec<u8>>> {
	let locations = paths.iter().map(|path| private_file_path(path)).collect::<Result<Vec<_>>>()?;
	let first =
		locations.first().ok_or_else(|| eyre::eyre!("Radar cache read set must not be empty"))?;
	let root = absolute_path_without_traversal(&first.root)?;

	for location in &locations[1..] {
		if absolute_path_without_traversal(&location.root)? != root {
			eyre::bail!("Radar cache read set must share one fixed cache root");
		}
	}

	let cache = PrivateCache::open_existing(&first.root)?;
	let lock = cache.lock()?;

	locations.iter().map(|location| lock.read(&location.relative)).collect()
}

pub(crate) fn write_private_file_atomic(path: &Path, payload: &[u8]) -> Result<()> {
	let location = private_file_path(path)?;
	let cache = PrivateCache::open_or_create(&location.root)?;
	let lock = cache.lock()?;

	lock.write_atomic(&location.relative, payload)
}

pub(crate) fn collect_private_json_files(directory: &Path) -> Result<Vec<PathBuf>> {
	let location = private_file_path(directory)?;
	let cache = PrivateCache::open_existing(&location.root)?;
	let lock = cache.lock()?;
	let mut files = Vec::new();

	collect_json_files_from_cache(lock.cache(), &location.relative, directory, &mut files)?;
	files.sort();

	Ok(files)
}

pub(crate) fn collect_private_json_files_if_present(directory: &Path) -> Result<Vec<PathBuf>> {
	match collect_private_json_files(directory) {
		Ok(files) => Ok(files),
		Err(error) if is_not_found(&error) => Ok(Vec::new()),
		Err(error) => Err(error),
	}
}

pub(crate) fn collect_private_json_files_under_lock(
	lock: &RadarCacheLock,
	directory: &Path,
) -> Result<Vec<PathBuf>> {
	let relative = lock.relative_path(directory)?;
	let mut files = Vec::new();

	collect_json_files_from_cache(lock.cache(), &relative, directory, &mut files)?;
	files.sort();

	Ok(files)
}

pub(crate) fn collect_private_json_files_under_lock_if_present(
	lock: &RadarCacheLock,
	directory: &Path,
) -> Result<Vec<PathBuf>> {
	match collect_private_json_files_under_lock(lock, directory) {
		Ok(files) => Ok(files),
		Err(error) if is_not_found(&error) => Ok(Vec::new()),
		Err(error) => Err(error),
	}
}

pub(crate) fn private_file_exists(path: &Path) -> Result<bool> {
	let location = private_file_path(path)?;
	let cache = match PrivateCache::open_existing(&location.root) {
		Ok(cache) => cache,
		Err(error) if is_not_found(&error) => return Ok(false),
		Err(error) => return Err(error),
	};

	Ok(cache.metadata(&location.relative)?.is_some())
}

pub(crate) fn private_file_exists_under_lock(lock: &RadarCacheLock, path: &Path) -> Result<bool> {
	let relative = lock.relative_path(path)?;

	Ok(lock.cache().metadata(&relative)?.is_some())
}

#[cfg(test)]
pub(crate) fn read_private_file_bounded_after_metadata(
	path: &Path,
	max_bytes: u64,
	after_metadata: impl FnOnce(),
) -> Result<Vec<u8>> {
	let location = private_file_path(path)?;
	let cache = PrivateCache::open_existing(&location.root)?;

	cache.read_bounded_with(&location.relative, max_bytes, after_metadata)
}

fn collect_json_files_from_cache(
	cache: &PrivateCache,
	relative: &Path,
	display_path: &Path,
	files: &mut Vec<PathBuf>,
) -> Result<()> {
	for entry in cache.entries(relative)? {
		let child_relative = relative.join(&entry.name);
		let child_display = display_path.join(&entry.name);

		match entry.kind {
			PrivateEntryKind::Directory => {
				collect_json_files_from_cache(cache, &child_relative, &child_display, files)?;
			},
			PrivateEntryKind::File
				if child_display.extension().is_some_and(|extension| extension == "json") =>
			{
				files.push(child_display);
			},
			PrivateEntryKind::File => {},
		}
	}

	Ok(())
}

#[cfg(test)]
pub(crate) fn ensure_private_directory(path: &Path) -> Result<()> {
	reject_unsafe_components(path)?;
	let components = path.components().collect::<Vec<_>>();

	if let Some(index) = components.windows(CACHE_MARKER.len()).position(|window| {
		window.iter().zip(CACHE_MARKER).all(|(actual, expected)| {
			matches!(actual, Component::Normal(value) if *value == OsStr::new(expected))
		})
	}) {
		let marker_end = index + CACHE_MARKER.len();
		let root = components[..marker_end].iter().fold(PathBuf::new(), |mut path, component| {
			path.push(component.as_os_str());
			path
		});
		let relative =
			components[marker_end..].iter().fold(PathBuf::new(), |mut path, component| {
				path.push(component.as_os_str());
				path
			});
		let cache = PrivateCache::open_or_create(&root)?;

		cache.create_directory_all(&relative)?;

		return Ok(());
	}

	drop(PrivateCache::open_or_create(path)?);

	Ok(())
}

#[cfg(test)]
pub(crate) fn create_private_test_directory(parent_path: &Path) -> Result<PrivateTestDirectory> {
	create_private_test_directory_with(parent_path, || {})
}

#[cfg(test)]
pub(crate) fn create_private_test_directory_with(
	parent_path: &Path,
	after_parent_open: impl FnOnce(),
) -> Result<PrivateTestDirectory> {
	let resolved_parent = parent_path.canonicalize()?;
	let parent = open_test_parent_path(&resolved_parent)?;
	let parent_identity = validate_test_parent_directory(&parent)?;

	verify_test_parent_binding(&resolved_parent, &parent, &parent_identity)?;
	after_parent_open();
	verify_test_parent_binding(&resolved_parent, &parent, &parent_identity)?;

	let name = test_temporary_name()?;
	if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), PRIVATE_DIR_MODE as libc::mode_t) }
		== -1
	{
		return Err(std::io::Error::last_os_error().into());
	}

	let result = (|| -> Result<(PathBuf, File, DirectoryIdentity)> {
		let directory = open_directory_at(parent.as_raw_fd(), &name)?;
		let identity = validate_private_directory(&directory, "test directory")?;

		parent.sync_all()?;
		verify_test_parent_binding(&resolved_parent, &parent, &parent_identity)?;

		let name = OsStr::from_bytes(name.to_bytes());

		Ok((resolved_parent.join(name), directory, identity))
	})();

	match result {
		Ok((path, directory, identity)) => Ok(PrivateTestDirectory {
			parent_path: resolved_parent,
			parent,
			parent_identity,
			name,
			path,
			directory,
			identity,
		}),
		Err(error) => {
			if let Ok(directory) = open_directory_at(parent.as_raw_fd(), &name)
				&& let Ok(identity) = directory_identity(&directory, "partial test directory")
			{
				let _ = remove_test_directory_contents(&directory);
				if verify_directory_binding_at(
					parent.as_raw_fd(),
					&name,
					&identity,
					"partial test directory",
				)
				.is_ok()
				{
					unsafe {
						libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR);
					}
				}
			}

			Err(error)
		},
	}
}

#[cfg(test)]
pub(crate) fn create_private_file(path: &Path) -> Result<File> {
	let location = private_file_path(path)?;

	ensure_private_directory(&location.root)?;

	let cache = PrivateCache::open_existing(&location.root)?;

	cache.create_new_file(&location.relative)
}

pub(crate) fn is_radar_cache_path(path: &Path) -> bool {
	path.components().collect::<Vec<_>>().windows(CACHE_MARKER.len()).any(|window| {
		window.iter().zip(CACHE_MARKER).all(|(actual, expected)| {
			matches!(actual, Component::Normal(value) if *value == OsStr::new(expected))
		})
	})
}

#[cfg(test)]
pub(crate) fn simulate_wrong_owner_error(_path: &Path) -> Result<()> {
	let expected_uid = unsafe { libc::geteuid() };

	validate_owner_mode_link(
		expected_uid.saturating_add(1),
		PRIVATE_FILE_MODE,
		1,
		PRIVATE_FILE_MODE,
		"file",
		true,
	)
}
