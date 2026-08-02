use std::{
	env,
	ffi::{OsStr, OsString},
	fs::{self, File},
	io::{Read as _, Write as _},
	os::unix::{
		ffi::OsStringExt as _,
		fs::{MetadataExt as _, PermissionsExt as _},
	},
	path::{Component, Path, PathBuf},
	sync::OnceLock,
};

use rustix::{
	fs::{self as unix_fs, AtFlags, Dir, Mode, OFlags},
	io::Errno,
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::prelude::{Result, eyre};

const MAX_PRIVATE_JSON_BYTES: u64 = 1024 * 1024;
const MAX_PRIVATE_JSON_FILES: usize = 4096;
const MAX_PRIVATE_TRAVERSAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PRIVATE_TRAVERSAL_ENTRIES: usize = 8192;
const PRIVATE_DIRECTORY_MODE: u16 = 0o700;
const PRIVATE_FILE_MODE: u16 = 0o600;

struct PrivateDirectory {
	file: File,
	path: PathBuf,
	private_anchor_found: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct PrivateFileIdentity {
	dev: u64,
	ino: u64,
	len: u64,
	mtime: i64,
	mtime_nsec: i64,
	ctime: i64,
	ctime_nsec: i64,
}

pub(crate) struct PinnedPrivateJsonFile {
	file: File,
	identity: PrivateFileIdentity,
	name: OsString,
	parent: PrivateDirectory,
	path: PathBuf,
	pub(crate) payload: Value,
}

#[derive(Default)]
struct TraversalLimits {
	entry_count: usize,
	json_bytes: u64,
}

pub(crate) fn repo_root() -> Result<PathBuf> {
	static ROOT: OnceLock<PathBuf> = OnceLock::new();

	if let Some(root) = ROOT.get() {
		return Ok(root.clone());
	}

	let current = env::current_dir()?;
	let root = current
		.ancestors()
		.find(|candidate| {
			candidate.join("automations/portfolio.toml").is_file()
				&& candidate.join("apps/decodex-publisher/src/lib.rs").is_file()
		})
		.map(Path::to_path_buf)
		.ok_or_else(|| eyre::eyre!("could not locate repository root"))?;
	let root = clean_absolute_path(&root)?;
	let _ = ROOT.set(root.clone());

	Ok(root)
}

#[cfg(test)]
pub(crate) fn repo_local_test_directory(prefix: &str) -> tempfile::TempDir {
	let repo_root = repo_root().expect("repository root");
	let target = repo_root.join("target");
	let configured = env::var_os("DECODEX_VALIDATION_REPO_OUTPUT")
		.map(PathBuf::from)
		.unwrap_or_else(|| target.clone());
	let configured = clean_absolute_path(&configured).expect("repo-local test output path");
	if env::var_os("DECODEX_CANDIDATE_SANDBOX").as_deref() == Some(OsStr::new("1")) {
		validate_sandbox_test_output(&target, &configured)
			.expect("sandboxed repo-local test output directory");
	} else {
		assert!(
			configured == target || configured.parent() == Some(target.as_path()),
			"repo-local test output must be target or one direct child"
		);
		ensure_private_directory(&configured).expect("repo-local test output directory");
	}
	tempfile::Builder::new()
		.prefix(prefix)
		.tempdir_in(configured)
		.expect("repo-local temporary directory")
}

#[cfg(test)]
fn validate_sandbox_test_output(target: &Path, configured: &Path) -> Result<()> {
	if configured == target || configured.parent() != Some(target) {
		return Err(eyre::eyre!(
			"sandboxed repo-local test output must be one direct child of target"
		));
	}
	let metadata = fs::symlink_metadata(configured)?;
	if !metadata.is_dir()
		|| metadata.uid() != current_uid()
		|| metadata.permissions().mode() & 0o7777 != u32::from(PRIVATE_DIRECTORY_MODE)
	{
		return Err(eyre::eyre!(
			"sandboxed repo-local test output must be an owned mode-0700 directory"
		));
	}

	Ok(())
}

pub(crate) fn resolve_against(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() { path.to_path_buf() } else { root.join(path) }
}

pub(crate) fn path_arg(root: &Path, path: &Path) -> String {
	path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

pub(crate) fn load_json(path: &Path) -> Result<Value> {
	Ok(read_private_json_file(path, MAX_PRIVATE_JSON_BYTES, || {})?.0)
}

pub(crate) fn load_json_with_sha256(path: &Path) -> Result<(Value, String)> {
	load_json_with_sha256_bounded(path, MAX_PRIVATE_JSON_BYTES)
}

pub(crate) fn load_json_with_sha256_bounded(
	path: &Path,
	max_bytes: u64,
) -> Result<(Value, String)> {
	let (value, bytes, _) = read_private_json_file(path, max_bytes, || {})?;
	let digest = Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect();

	Ok((value, digest))
}

#[cfg(test)]
pub(crate) fn load_json_bytes_with_sha256_after_metadata(
	path: &Path,
	max_bytes: u64,
	after_metadata: impl FnOnce(),
) -> Result<(Value, Vec<u8>, String)> {
	let (value, bytes, _) = read_private_json_file(path, max_bytes, after_metadata)?;
	let digest = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();

	Ok((value, bytes, digest))
}

pub(crate) fn write_new_json(path: &Path, payload: &Value) -> Result<()> {
	let (parent_path, file_name) = parent_and_name(path)?;
	let parent = open_private_directory(&parent_path, true)?;
	write_new_json_in_parent(&parent, &file_name, path, payload)
}

fn write_new_json_in_parent(
	parent: &PrivateDirectory,
	file_name: &OsStr,
	path: &Path,
	payload: &Value,
) -> Result<()> {
	let temporary_name = temporary_name(file_name)?;
	let mut file = create_private_file(parent, &temporary_name)?;
	file.write_all(serde_json::to_string_pretty(payload)?.as_bytes())?;
	file.write_all(b"\n")?;
	file.sync_all()?;
	validate_private_json_metadata(path, &file.metadata()?)?;
	drop(file);

	let linked =
		unix_fs::linkat(&parent.file, &temporary_name, &parent.file, file_name, AtFlags::empty());
	let cleanup = unix_fs::unlinkat(&parent.file, &temporary_name, AtFlags::empty());
	if let Err(error) = linked {
		if cleanup.is_err() {
			return Err(eyre::eyre!(
				"failed to publish and clean temporary JSON file {}: {error}",
				path.display()
			));
		}
		if error == Errno::EXIST {
			return Err(eyre::eyre!("refusing to overwrite existing file: {}", path.display()));
		}

		return Err(eyre::eyre!("failed to publish {}: {error}", path.display()));
	}
	cleanup?;
	parent.file.sync_all()?;
	validate_named_private_file(parent, file_name, path)?;

	Ok(())
}

pub(crate) fn replace_existing_json(path: &Path, expected: &Value, payload: &Value) -> Result<()> {
	let (parent_path, file_name) = parent_and_name(path)?;
	let parent = open_private_directory(&parent_path, false)?;
	let mut existing = open_named_private_file(&parent, &file_name, path)?;
	let existing_metadata = existing.metadata()?;
	if read_json(&mut existing, path)? != *expected {
		return Err(eyre::eyre!("existing JSON changed before replacement"));
	}

	let temporary_name = temporary_name(&file_name)?;
	let mut replacement = create_private_file(&parent, &temporary_name)?;
	replacement.write_all(serde_json::to_string_pretty(payload)?.as_bytes())?;
	replacement.write_all(b"\n")?;
	replacement.sync_all()?;
	validate_private_json_metadata(path, &replacement.metadata()?)?;
	drop(replacement);

	let current = match open_named_private_file(&parent, &file_name, path) {
		Ok(file) => file,
		Err(error) => {
			let _ = unix_fs::unlinkat(&parent.file, &temporary_name, AtFlags::empty());
			return Err(error);
		},
	};
	if !same_file(&existing_metadata, &current.metadata()?) {
		let _ = unix_fs::unlinkat(&parent.file, &temporary_name, AtFlags::empty());
		return Err(eyre::eyre!("existing JSON changed during replacement"));
	}
	if let Err(error) = unix_fs::renameat(&parent.file, &temporary_name, &parent.file, &file_name) {
		let _ = unix_fs::unlinkat(&parent.file, &temporary_name, AtFlags::empty());
		return Err(eyre::eyre!("failed to replace {}: {error}", path.display()));
	}
	parent.file.sync_all()?;
	validate_named_private_file(&parent, &file_name, path)?;

	Ok(())
}

pub(crate) fn require_contained_regular_file(path: &Path, root: &Path) -> Result<()> {
	let root = clean_absolute_path(root)?;
	let path = clean_absolute_path(path)?;
	if !path.starts_with(&root) {
		return Err(eyre::eyre!("file must stay under its configured directory"));
	}
	let relative = path
		.strip_prefix(&root)
		.map_err(|_| eyre::eyre!("file must stay under its configured directory"))?;
	if relative.components().any(|component| !matches!(component, Component::Normal(_))) {
		return Err(eyre::eyre!("file path has unsupported components"));
	}
	let _ = open_private_directory(&root, false)?;
	let _ = open_private_file(&path)?;

	Ok(())
}

pub(crate) fn collect_json_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
	let mut files = Vec::new();
	let mut limits = TraversalLimits::default();

	for path in paths {
		let display_path = path.clone();
		let absolute = clean_absolute_path(path)?;
		let opened = match open_path(&absolute) {
			Ok(opened) => opened,
			Err(error) if error.downcast_ref::<Errno>() == Some(&Errno::NOENT) => continue,
			Err(error) => return Err(error),
		};
		collect_opened_path(opened, display_path, &mut files, &mut limits)?;
	}

	files.sort();

	Ok(files)
}

pub(crate) fn ensure_private_directory(path: &Path) -> Result<()> {
	let root = repo_root()?;
	let path = clean_absolute_path(path)?;
	if !path.starts_with(&root) {
		#[cfg(not(test))]
		return Err(eyre::eyre!("private state directory escaped the repository"));
	}
	let _ = open_private_directory(&path, true)?;

	Ok(())
}

pub(crate) fn open_private_directory_descriptor(path: &Path, create: bool) -> Result<File> {
	Ok(open_private_directory(path, create)?.file)
}

pub(crate) fn open_or_create_private_lock(path: &Path) -> Result<File> {
	let (parent_path, file_name) = parent_and_name(path)?;
	let parent = open_private_directory(&parent_path, true)?;
	let fd = unix_fs::openat(
		&parent.file,
		&file_name,
		OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW,
		Mode::from_bits_retain(PRIVATE_FILE_MODE),
	)
	.map_err(|error| eyre::eyre!("private lock path is not safe: {error}"))?;
	let file = File::from(fd);
	file.set_permissions(fs::Permissions::from_mode(PRIVATE_FILE_MODE.into()))?;
	let metadata = file.metadata()?;
	if !metadata.is_file()
		|| metadata.uid() != current_uid()
		|| metadata.nlink() != 1
		|| metadata.permissions().mode() & 0o777 != u32::from(PRIVATE_FILE_MODE)
	{
		return Err(eyre::eyre!("private lock path is not an owned mode-0600 file"));
	}

	Ok(file)
}

impl PrivateFileIdentity {
	pub(crate) fn from_metadata(metadata: &fs::Metadata) -> Self {
		Self {
			dev: metadata.dev(),
			ino: metadata.ino(),
			len: metadata.len(),
			mtime: metadata.mtime(),
			mtime_nsec: metadata.mtime_nsec(),
			ctime: metadata.ctime(),
			ctime_nsec: metadata.ctime_nsec(),
		}
	}
}

enum OpenedPath {
	Directory(PrivateDirectory),
	File(File),
}

fn open_path(path: &Path) -> Result<OpenedPath> {
	let (parent_path, name) = parent_and_name(path)?;
	let parent = open_private_directory(&parent_path, false)?;
	let fd = unix_fs::openat(
		&parent.file,
		&name,
		OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
		Mode::empty(),
	)?;
	let file = File::from(fd);
	let metadata = file.metadata()?;
	if metadata.is_dir() {
		let private_anchor_found =
			validate_directory_metadata(path, &metadata, parent.private_anchor_found)?;
		return Ok(OpenedPath::Directory(PrivateDirectory {
			file,
			path: path.to_path_buf(),
			private_anchor_found,
		}));
	}
	if metadata.is_file() {
		return Ok(OpenedPath::File(file));
	}

	Err(eyre::eyre!("private JSON traversal found an unsupported filesystem entry"))
}

fn collect_opened_path(
	opened: OpenedPath,
	display_path: PathBuf,
	files: &mut Vec<PathBuf>,
	limits: &mut TraversalLimits,
) -> Result<()> {
	match opened {
		OpenedPath::File(file) => {
			if display_path.extension().and_then(OsStr::to_str) == Some("json") {
				let metadata = file.metadata()?;
				validate_private_json_metadata(&display_path, &metadata)?;
				push_bounded(files, display_path, metadata.len(), limits)?;
			}
		},
		OpenedPath::Directory(directory) =>
			collect_directory_json_files(directory, display_path, files, limits)?,
	}

	Ok(())
}

fn collect_directory_json_files(
	directory: PrivateDirectory,
	display_path: PathBuf,
	files: &mut Vec<PathBuf>,
	limits: &mut TraversalLimits,
) -> Result<()> {
	let mut entries = Vec::new();
	for entry in Dir::read_from(&directory.file)? {
		let entry = entry?;
		limits.entry_count += 1;
		if limits.entry_count > MAX_PRIVATE_TRAVERSAL_ENTRIES {
			return Err(eyre::eyre!(
				"private JSON traversal exceeds {MAX_PRIVATE_TRAVERSAL_ENTRIES} entries"
			));
		}
		entries.push(OsString::from_vec(entry.file_name().to_bytes().to_vec()));
	}
	entries.retain(|name| name != "." && name != "..");
	entries.sort();

	for name in entries {
		let child_display = display_path.join(&name);
		let child_path = directory.path.join(&name);
		let fd = unix_fs::openat(
			&directory.file,
			&name,
			OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
			Mode::empty(),
		)
		.map_err(|error| {
			eyre::eyre!(
				"private JSON traversal found a symlink or changed entry {}: {error}",
				child_display.display()
			)
		})?;
		let file = File::from(fd);
		let metadata = file.metadata()?;
		if metadata.is_dir() {
			let private_anchor_found = validate_directory_metadata(
				&child_path,
				&metadata,
				directory.private_anchor_found,
			)?;
			collect_directory_json_files(
				PrivateDirectory { file, path: child_path, private_anchor_found },
				child_display,
				files,
				limits,
			)?;
		} else if metadata.is_file() {
			if child_display.extension().and_then(OsStr::to_str) == Some("json") {
				validate_private_json_metadata(&child_display, &metadata)?;
				push_bounded(files, child_display, metadata.len(), limits)?;
			}
		} else {
			return Err(eyre::eyre!(
				"private JSON traversal found an unsupported filesystem entry"
			));
		}
	}

	Ok(())
}

fn push_bounded(
	files: &mut Vec<PathBuf>,
	path: PathBuf,
	size: u64,
	limits: &mut TraversalLimits,
) -> Result<()> {
	if files.len() >= MAX_PRIVATE_JSON_FILES {
		return Err(eyre::eyre!("private JSON traversal exceeds {MAX_PRIVATE_JSON_FILES} files"));
	}
	limits.json_bytes = limits
		.json_bytes
		.checked_add(size)
		.ok_or_else(|| eyre::eyre!("private JSON traversal byte count overflowed"))?;
	if limits.json_bytes > MAX_PRIVATE_TRAVERSAL_BYTES {
		return Err(eyre::eyre!(
			"private JSON traversal exceeds {MAX_PRIVATE_TRAVERSAL_BYTES} bytes"
		));
	}
	files.push(path);

	Ok(())
}

fn open_private_file(path: &Path) -> Result<File> {
	let (parent_path, file_name) = parent_and_name(path)?;
	let parent = open_private_directory(&parent_path, false)?;
	open_named_private_file(&parent, &file_name, path)
}

fn open_named_private_file(
	parent: &PrivateDirectory,
	file_name: &OsStr,
	path: &Path,
) -> Result<File> {
	open_named_private_file_optional(parent, file_name, path)?
		.ok_or_else(|| eyre::eyre!("private JSON does not exist: {}", path.display()))
}

fn open_named_private_file_optional(
	parent: &PrivateDirectory,
	file_name: &OsStr,
	path: &Path,
) -> Result<Option<File>> {
	let fd = match unix_fs::openat(
		&parent.file,
		file_name,
		OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
		Mode::empty(),
	) {
		Ok(fd) => fd,
		Err(Errno::NOENT) => return Ok(None),
		Err(error) => {
			return Err(eyre::eyre!(
				"private JSON must be a regular non-symlink file {}: {error}",
				path.display()
			));
		},
	};
	let file = File::from(fd);
	validate_private_json_metadata(path, &file.metadata()?)?;

	Ok(Some(file))
}

fn create_private_file(parent: &PrivateDirectory, file_name: &OsStr) -> Result<File> {
	let fd = unix_fs::openat(
		&parent.file,
		file_name,
		OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
		Mode::from_bits_retain(PRIVATE_FILE_MODE),
	)?;
	let file = File::from(fd);
	file.set_permissions(fs::Permissions::from_mode(PRIVATE_FILE_MODE.into()))?;

	Ok(file)
}

fn validate_named_private_file(
	parent: &PrivateDirectory,
	file_name: &OsStr,
	path: &Path,
) -> Result<()> {
	let file = open_named_private_file(parent, file_name, path)?;
	validate_private_json_metadata(path, &file.metadata()?)
}

fn read_json(file: &mut File, path: &Path) -> Result<Value> {
	Ok(read_json_bytes_bounded(file, path, MAX_PRIVATE_JSON_BYTES)?.0)
}

fn read_json_bytes_bounded(
	file: &mut File,
	path: &Path,
	max_bytes: u64,
) -> Result<(Value, Vec<u8>)> {
	let before = file.metadata()?;
	validate_private_json_metadata(path, &before)?;
	if before.len() > max_bytes {
		return Err(eyre::eyre!("private JSON exceeds its bounded read limit"));
	}
	let mut bytes = Vec::with_capacity(usize::try_from(before.len()).unwrap_or(0));
	file.take(max_bytes + 1).read_to_end(&mut bytes)?;
	let after = file.metadata()?;
	if bytes.len() as u64 != before.len()
		|| !same_file(&before, &after)
		|| before.modified()? != after.modified()?
	{
		return Err(eyre::eyre!("private JSON changed during read: {}", path.display()));
	}
	let payload =
		std::str::from_utf8(&bytes).map_err(|_| eyre::eyre!("{} is not UTF-8", path.display()))?;

	let value = serde_json::from_str(payload)
		.map_err(|error| eyre::eyre!("failed to parse {} as JSON: {error}", path.display()))?;

	Ok((value, bytes))
}

fn read_private_json_file(
	path: &Path,
	max_bytes: u64,
	after_metadata: impl FnOnce(),
) -> Result<(Value, Vec<u8>, PrivateFileIdentity)> {
	let (parent_path, name) = parent_and_name(path)?;
	let parent = open_private_directory(&parent_path, false)?;
	let (value, bytes, identity, _) =
		read_named_private_json_file(&parent, &name, path, max_bytes, after_metadata)?;

	Ok((value, bytes, identity))
}

fn read_named_private_json_file(
	parent: &PrivateDirectory,
	name: &OsStr,
	path: &Path,
	max_bytes: u64,
	after_metadata: impl FnOnce(),
) -> Result<(Value, Vec<u8>, PrivateFileIdentity, File)> {
	let mut file = open_named_private_file(parent, name, path)?;
	let initial = PrivateFileIdentity::from_metadata(&file.metadata()?);
	if initial.len > max_bytes {
		return Err(eyre::eyre!("private JSON exceeds its bounded read limit"));
	}
	after_metadata();
	let read_limit = max_bytes
		.checked_add(1)
		.ok_or_else(|| eyre::eyre!("private JSON bounded read limit is too large"))?;
	let mut bytes = Vec::with_capacity(usize::try_from(initial.len).unwrap_or(0));
	std::io::Read::by_ref(&mut file).take(read_limit).read_to_end(&mut bytes)?;
	if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
		return Err(eyre::eyre!("private JSON exceeds its bounded read limit"));
	}
	let held = PrivateFileIdentity::from_metadata(&file.metadata()?);
	let current = open_named_private_file(parent, name, path)?;
	let current_identity = PrivateFileIdentity::from_metadata(&current.metadata()?);
	if held != initial
		|| current_identity != initial
		|| u64::try_from(bytes.len()).unwrap_or(u64::MAX) != initial.len
	{
		return Err(eyre::eyre!("private JSON identity changed during read: {}", path.display()));
	}
	verify_private_directory_current_path(parent)?;
	let payload =
		std::str::from_utf8(&bytes).map_err(|_| eyre::eyre!("{} is not UTF-8", path.display()))?;
	let value = serde_json::from_str(payload)
		.map_err(|error| eyre::eyre!("failed to parse {} as JSON: {error}", path.display()))?;
	verify_private_directory_current_path(parent)?;

	Ok((value, bytes, initial, file))
}

impl PinnedPrivateJsonFile {
	pub(crate) fn open(path: &Path, max_bytes: u64) -> Result<Self> {
		let (parent_path, name) = parent_and_name(path)?;
		let parent = open_private_directory(&parent_path, false)?;
		let (payload, _, identity, file) =
			read_named_private_json_file(&parent, &name, path, max_bytes, || {})?;

		Ok(Self { file, identity, name, parent, path: path.to_path_buf(), payload })
	}

	pub(crate) fn unlink(self) -> Result<()> {
		verify_private_directory_current_path(&self.parent)?;
		let current = open_named_private_file(&self.parent, &self.name, &self.path)?;
		if PrivateFileIdentity::from_metadata(&current.metadata()?) != self.identity
			|| PrivateFileIdentity::from_metadata(&self.file.metadata()?) != self.identity
		{
			return Err(eyre::eyre!(
				"private JSON changed before cleanup: {}",
				self.path.display()
			));
		}
		verify_private_directory_current_path(&self.parent)?;
		unix_fs::unlinkat(&self.parent.file, &self.name, AtFlags::empty())?;
		self.parent.file.sync_all()?;
		verify_private_directory_current_path(&self.parent)?;

		Ok(())
	}
}

fn open_private_directory(path: &Path, create: bool) -> Result<PrivateDirectory> {
	let path = clean_absolute_path(path)?;
	#[cfg(test)]
	if let Some(current) = open_sandbox_private_root(&path, create)? {
		let relative = path
			.strip_prefix(&current.path)
			.map_err(|_| eyre::eyre!("sandboxed private path escaped its pinned root"))?;
		return descend_private_directory(current, relative.components(), create);
	}
	let root_fd = unix_fs::open(
		"/",
		OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
		Mode::empty(),
	)?;
	let mut current = PrivateDirectory {
		file: File::from(root_fd),
		path: PathBuf::from("/"),
		private_anchor_found: false,
	};
	let metadata = current.file.metadata()?;
	current.private_anchor_found = validate_directory_metadata(&current.path, &metadata, false)?;

	descend_private_directory(current, path.components(), create)
}

fn verify_private_directory_current_path(directory: &PrivateDirectory) -> Result<()> {
	let expected = directory.file.metadata()?;
	let current = open_private_directory(&directory.path, false)?;
	let actual = current.file.metadata()?;
	if (expected.dev(), expected.ino()) != (actual.dev(), actual.ino()) {
		return Err(eyre::eyre!(
			"private directory path changed after descriptor pin: {}",
			directory.path.display()
		));
	}

	Ok(())
}

#[cfg(test)]
fn open_sandbox_private_root(path: &Path, create: bool) -> Result<Option<PrivateDirectory>> {
	if env::var_os("DECODEX_CANDIDATE_SANDBOX").as_deref() != Some(OsStr::new("1")) {
		return Ok(None);
	}
	let roots = ["DECODEX_VALIDATION_REPO_OUTPUT", "TMPDIR"]
		.into_iter()
		.filter_map(env::var_os)
		.map(PathBuf::from)
		.map(|root| clean_absolute_path(&root))
		.collect::<Result<Vec<_>>>()?;
	if matching_sandbox_root(path, roots.clone()).is_some() {
		return Ok(Some(open_pinned_sandbox_private_root(path, roots)?));
	}
	let candidate = repo_root()?;
	if path == candidate || path.starts_with(&candidate) {
		if create {
			return Err(eyre::eyre!("sandboxed private path is outside its writable pinned roots"));
		}
		return Ok(Some(open_pinned_sandbox_read_root(candidate)?));
	}

	Err(eyre::eyre!("sandboxed private path is outside its pinned roots"))
}

#[cfg(test)]
fn open_pinned_sandbox_private_root(path: &Path, roots: Vec<PathBuf>) -> Result<PrivateDirectory> {
	let root = matching_sandbox_root(path, roots)
		.ok_or_else(|| eyre::eyre!("sandboxed private path is outside its pinned roots"))?;
	open_pinned_sandbox_root(root, true)
}

#[cfg(test)]
fn matching_sandbox_root(path: &Path, mut roots: Vec<PathBuf>) -> Option<PathBuf> {
	roots.sort_by_key(|root| root.components().count());
	roots.into_iter().rev().find(|root| path == root || path.starts_with(root))
}

#[cfg(test)]
fn open_pinned_sandbox_read_root(root: PathBuf) -> Result<PrivateDirectory> {
	open_pinned_sandbox_root(root, false)
}

#[cfg(test)]
fn open_pinned_sandbox_root(root: PathBuf, exact_private: bool) -> Result<PrivateDirectory> {
	let before = fs::symlink_metadata(&root)?;
	let private_anchor_found = if exact_private {
		validate_exact_sandbox_private_directory_metadata(&before)?;
		true
	} else {
		validate_directory_metadata(&root, &before, false)?
	};
	let fd = unix_fs::open(
		&root,
		OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
		Mode::empty(),
	)?;
	let file = File::from(fd);
	let after = file.metadata()?;
	if exact_private {
		validate_exact_sandbox_private_directory_metadata(&after)?;
	} else if validate_directory_metadata(&root, &after, false)? != private_anchor_found {
		return Err(eyre::eyre!("sandboxed read root authority changed while opening"));
	}
	if (before.dev(), before.ino()) != (after.dev(), after.ino()) {
		return Err(eyre::eyre!("sandboxed private root changed while opening"));
	}

	Ok(PrivateDirectory { file, path: root, private_anchor_found })
}

#[cfg(test)]
fn validate_exact_sandbox_private_directory_metadata(metadata: &fs::Metadata) -> Result<()> {
	if !metadata.is_dir()
		|| metadata.uid() != current_uid()
		|| metadata.permissions().mode() & 0o7777 != u32::from(PRIVATE_DIRECTORY_MODE)
	{
		return Err(eyre::eyre!(
			"sandboxed private root is not an owned exact mode-0700 directory"
		));
	}

	Ok(())
}

fn descend_private_directory<'a>(
	mut current: PrivateDirectory,
	components: impl Iterator<Item = Component<'a>>,
	create: bool,
) -> Result<PrivateDirectory> {
	for component in components {
		let Component::Normal(name) = component else {
			continue;
		};
		let child_path = current.path.join(name);
		let (fd, created) = match unix_fs::openat(
			&current.file,
			name,
			OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
			Mode::empty(),
		) {
			Ok(fd) => (fd, false),
			Err(Errno::NOENT) if create => {
				if !current.private_anchor_found {
					return Err(eyre::eyre!(
						"private state directory has no trusted user-owned anchor"
					));
				}
				unix_fs::mkdirat(
					&current.file,
					name,
					Mode::from_bits_retain(PRIVATE_DIRECTORY_MODE),
				)?;
				(
					unix_fs::openat(
						&current.file,
						name,
						OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
						Mode::empty(),
					)?,
					true,
				)
			},
			Err(Errno::NOENT) => return Err(Errno::NOENT.into()),
			Err(error) => {
				return Err(eyre::eyre!(
					"private state directory must not contain a symlink or unsafe component {}: {error}",
					child_path.display()
				));
			},
		};
		let file = File::from(fd);
		if created {
			file.set_permissions(fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE.into()))?;
		}
		let metadata = file.metadata()?;
		let private_anchor_found =
			validate_directory_metadata(&child_path, &metadata, current.private_anchor_found)?;
		current = PrivateDirectory { file, path: child_path, private_anchor_found };
	}

	Ok(current)
}

fn validate_directory_metadata(
	path: &Path,
	metadata: &fs::Metadata,
	private_anchor_found: bool,
) -> Result<bool> {
	let mode = metadata.permissions().mode();
	let uid = metadata.uid();
	let trusted_sticky_root =
		!private_anchor_found && uid == 0 && mode & 0o022 != 0 && mode & 0o1000 != 0;
	if !metadata.is_dir() || mode & 0o022 != 0 && !trusted_sticky_root {
		return Err(eyre::eyre!(
			"private state directory owner or mode is unsafe: {}",
			path.display()
		));
	}
	let current_uid = current_uid();
	if private_anchor_found && uid != current_uid
		|| !private_anchor_found && !matches!(uid, 0) && uid != current_uid
	{
		return Err(eyre::eyre!(
			"private state directory owner or mode is unsafe: {}",
			path.display()
		));
	}

	Ok(private_anchor_found || uid == current_uid)
}

fn validate_private_json_metadata(path: &Path, metadata: &fs::Metadata) -> Result<()> {
	if !metadata.is_file()
		|| metadata.len() == 0
		|| metadata.len() > MAX_PRIVATE_JSON_BYTES
		|| metadata.uid() != current_uid()
		|| metadata.nlink() != 1
		|| metadata.permissions().mode() & 0o777 != u32::from(PRIVATE_FILE_MODE)
	{
		return Err(eyre::eyre!("private JSON metadata is invalid: {}", path.display()));
	}

	Ok(())
}

fn parent_and_name(path: &Path) -> Result<(PathBuf, OsString)> {
	let path = clean_absolute_path(path)?;
	let parent =
		path.parent().ok_or_else(|| eyre::eyre!("private path must have a parent"))?.to_path_buf();
	let file_name = path
		.file_name()
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("private path must have a filename"))?
		.to_os_string();

	Ok((parent, file_name))
}

fn clean_absolute_path(path: &Path) -> Result<PathBuf> {
	let source =
		if path.is_absolute() { path.to_path_buf() } else { env::current_dir()?.join(path) };
	clean_absolute_path_inner(&source, 0)
}

fn clean_absolute_path_inner(source: &Path, depth: usize) -> Result<PathBuf> {
	if depth > 8 {
		return Err(eyre::eyre!("private path has too many system symlink components"));
	}
	let components = source.components().collect::<Vec<_>>();
	let mut clean = PathBuf::from("/");
	let mut root_owned_prefix = true;
	for (index, component) in components.iter().enumerate() {
		match component {
			Component::RootDir | Component::CurDir => {},
			Component::Normal(value) => {
				let candidate = clean.join(value);
				if root_owned_prefix {
					match fs::symlink_metadata(&candidate) {
						Ok(metadata) if metadata.file_type().is_symlink() => {
							if metadata.uid() != 0 {
								return Err(eyre::eyre!(
									"private path contains an untrusted symlink: {}",
									candidate.display()
								));
							}
							let target = fs::read_link(&candidate)?;
							let mut resolved =
								if target.is_absolute() { target } else { clean.join(target) };
							for remaining in &components[index + 1..] {
								resolved.push(remaining.as_os_str());
							}
							return clean_absolute_path_inner(&resolved, depth + 1);
						},
						Ok(metadata) => {
							root_owned_prefix =
								metadata.uid() == 0
									&& metadata.is_dir() && metadata.permissions().mode() & 0o022 == 0;
						},
						Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
							root_owned_prefix = false;
						},
						Err(error) => return Err(error.into()),
					}
				}
				clean.push(value);
			},
			Component::ParentDir | Component::Prefix(_) => {
				return Err(eyre::eyre!("private path has unsupported components"));
			},
		}
	}

	Ok(clean)
}

fn temporary_name(file_name: &OsStr) -> Result<OsString> {
	let file_name =
		file_name.to_str().ok_or_else(|| eyre::eyre!("private filename must be UTF-8"))?;
	let mut random = [0_u8; 16];
	getrandom::fill(&mut random).map_err(|_| eyre::eyre!("secure randomness is unavailable"))?;
	let suffix = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();

	Ok(OsString::from(format!(".{file_name}.{suffix}.tmp")))
}

fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
	left.dev() == right.dev()
		&& left.ino() == right.ino()
		&& left.len() == right.len()
		&& left.nlink() == right.nlink()
}

fn current_uid() -> u32 {
	unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests {
	use std::{
		ffi::CString,
		fs,
		fs::FileTimes,
		io::Write as _,
		os::unix::{
			ffi::OsStrExt as _,
			fs::{MetadataExt as _, PermissionsExt as _, symlink},
		},
		sync::mpsc,
		thread,
		time::{Duration, SystemTime},
	};

	use serde_json::json;

	use super::{
		PinnedPrivateJsonFile, load_json, load_json_bytes_with_sha256_after_metadata,
		open_pinned_sandbox_private_root, open_pinned_sandbox_read_root, open_private_directory,
		repo_local_test_directory, validate_sandbox_test_output, write_new_json_in_parent,
	};

	#[test]
	fn private_json_reads_reject_fifos_without_blocking_for_lineage_and_staging() {
		let temp = repo_local_test_directory("publisher-private-fifo-");
		let parent = temp.path().join(".agent/automations/decodex/cache/manager/staging");
		let placeholder = parent.join("placeholder.json");
		super::write_new_json(&placeholder, &json!({"ok": true})).expect("private parent fixture");
		fs::remove_file(placeholder).expect("placeholder removal");

		for (name, staging) in [("lineage.fifo", false), ("staging.fifo", true)] {
			let path = parent.join(name);
			let fifo = CString::new(path.as_os_str().as_bytes()).expect("FIFO path");
			assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
			let (sender, receiver) = mpsc::channel();
			let reader = thread::spawn(move || {
				let result = if staging {
					PinnedPrivateJsonFile::open(&path, 1024).map(|_| ())
				} else {
					load_json(&path).map(|_| ())
				};
				sender.send(result).expect("FIFO result delivery");
			});
			let error = receiver
				.recv_timeout(Duration::from_secs(2))
				.expect("private FIFO open must not block")
				.expect_err("FIFO must fail regular-file validation");
			reader.join().expect("FIFO reader should exit");
			assert!(!error.to_string().is_empty());
		}
	}

	#[test]
	fn private_json_reads_reject_symlinks_oversize_and_growth_past_max_plus_one() {
		let temp = repo_local_test_directory("publisher-private-bounds-");
		let parent = temp.path().join(".agent/automations/decodex/cache/social/x/candidates");
		let target = parent.join("target.json");
		let linked = parent.join("linked.json");
		super::write_new_json(&target, &json!({"value": 1})).expect("target fixture");
		symlink(&target, &linked).expect("symlink fixture");
		assert!(load_json(&linked).is_err());
		assert!(super::load_json_with_sha256_bounded(&target, 4).is_err());

		let growing = parent.join("growing.json");
		fs::write(&growing, b"{}\n").expect("growing fixture");
		fs::set_permissions(&growing, fs::Permissions::from_mode(0o600))
			.expect("growing fixture mode");
		let append = growing.clone();
		let error = load_json_bytes_with_sha256_after_metadata(&growing, 3, move || {
			let mut file =
				fs::OpenOptions::new().append(true).open(append).expect("append fixture");
			file.write_all(b" ").expect("grow fixture");
			file.sync_all().expect("sync growth");
		})
		.expect_err("max-plus-one read must reject growth");
		assert!(error.to_string().contains("bounded read limit"));
	}

	#[test]
	fn private_json_reads_reject_path_replacement_and_mtime_restored_rewrite() {
		let temp = repo_local_test_directory("publisher-private-identity-");
		let parent = temp.path().join(".agent/automations/decodex/cache/social/x/candidates");
		let path = parent.join("candidate.json");
		let displaced = parent.join("displaced.json");
		super::write_new_json(&path, &json!({"value": 1})).expect("candidate fixture");
		let replacement = path.clone();
		let error = load_json_bytes_with_sha256_after_metadata(&path, 1024, move || {
			fs::rename(&replacement, &displaced).expect("displace candidate");
			super::write_new_json(&replacement, &json!({"value": 1}))
				.expect("replacement candidate");
		})
		.expect_err("path replacement must fail identity revalidation");
		assert!(error.to_string().contains("identity changed during read"));

		let rewrite = parent.join("rewrite.json");
		super::write_new_json(&rewrite, &json!({"value": 1})).expect("rewrite fixture");
		let modified = SystemTime::now() - Duration::from_secs(60);
		let file = fs::OpenOptions::new().write(true).open(&rewrite).expect("rewrite descriptor");
		file.set_times(FileTimes::new().set_modified(modified)).expect("fixed mtime");
		let initial = file.metadata().expect("initial rewrite metadata");
		let initial_ctime = (initial.ctime(), initial.ctime_nsec());
		let rewrite_path = rewrite.clone();
		let error = load_json_bytes_with_sha256_after_metadata(&rewrite, 1024, move || {
			thread::sleep(Duration::from_millis(10));
			let mut file = fs::OpenOptions::new()
				.write(true)
				.truncate(true)
				.open(rewrite_path)
				.expect("rewrite file");
			file.write_all(b"{\n  \"value\": 2\n}\n").expect("same-size replacement");
			file.sync_all().expect("sync replacement");
			file.set_times(FileTimes::new().set_modified(modified)).expect("restore mtime");
			let changed = file.metadata().expect("changed metadata");
			assert_ne!((changed.ctime(), changed.ctime_nsec()), initial_ctime);
		})
		.expect_err("ctime must detect an mtime-restored rewrite");
		assert!(error.to_string().contains("identity changed during read"));
	}

	#[test]
	fn private_json_reads_reject_parent_rename_and_replacement() {
		let temp = repo_local_test_directory("publisher-private-parent-identity-");
		let parent = temp.path().join(".agent/automations/decodex/cache/social/x/candidates");
		let displaced = parent.with_file_name("displaced-candidates");
		let path = parent.join("candidate.json");
		super::write_new_json(&path, &json!({"value": 1})).expect("candidate fixture");
		let parent_for_hook = parent.clone();
		let displaced_for_hook = displaced.clone();
		let error = load_json_bytes_with_sha256_after_metadata(&path, 1024, move || {
			fs::rename(&parent_for_hook, &displaced_for_hook).expect("parent displacement");
			super::write_new_json(&parent_for_hook.join("candidate.json"), &json!({"value": 1}))
				.expect("parent replacement");
		})
		.expect_err("read must reject a displaced parent tree");
		assert!(error.to_string().contains("directory path changed"));
		assert!(displaced.join("candidate.json").exists());
		assert!(parent.join("candidate.json").exists());
	}

	#[test]
	fn staging_unlink_rejects_parent_rename_and_replacement() {
		let temp = repo_local_test_directory("publisher-staging-parent-identity-");
		let parent = temp.path().join(".agent/automations/decodex/cache/manager/staging");
		let displaced = parent.with_file_name("displaced-staging");
		let path = parent.join("staged.json");
		super::write_new_json(&path, &json!({"value": 1})).expect("staging fixture");
		let staged = PinnedPrivateJsonFile::open(&path, 1024).expect("pinned staging fixture");
		fs::rename(&parent, &displaced).expect("staging parent displacement");
		super::write_new_json(&path, &json!({"value": 1})).expect("staging parent replacement");

		let error = staged.unlink().expect_err("unlink must reject a displaced parent tree");
		assert!(error.to_string().contains("directory path changed"));
		assert!(displaced.join("staged.json").exists());
		assert!(path.exists());
	}

	#[test]
	fn sandbox_test_output_requires_an_exact_private_direct_child() {
		let temp = tempfile::tempdir().expect("temporary directory");
		let target = temp.path().join("target");
		let output = target.join("decodex-validation-test");
		fs::create_dir(&target).expect("target directory");
		fs::create_dir(&output).expect("output directory");
		fs::set_permissions(&output, fs::Permissions::from_mode(0o700))
			.expect("private output mode");

		validate_sandbox_test_output(&target, &output).expect("valid direct child");
		assert!(validate_sandbox_test_output(&target, &target).is_err());

		let outside = temp.path().join("outside");
		fs::create_dir(&outside).expect("outside directory");
		assert!(validate_sandbox_test_output(&target, &outside).is_err());

		fs::set_permissions(&output, fs::Permissions::from_mode(0o755))
			.expect("unsafe output mode");
		assert!(validate_sandbox_test_output(&target, &output).is_err());

		fs::set_permissions(&output, fs::Permissions::from_mode(0o1700))
			.expect("sticky output mode");
		assert!(validate_sandbox_test_output(&target, &output).is_err());

		fs::remove_dir(&output).expect("remove output");
		symlink(&outside, &output).expect("symlink output");
		assert!(validate_sandbox_test_output(&target, &output).is_err());
	}

	#[test]
	fn sandbox_private_root_is_opened_directly_and_rejects_unsafe_roots() {
		let temp = tempfile::tempdir().expect("temporary directory");
		let root = temp.path().join("root");
		let outside = temp.path().join("outside");
		fs::create_dir(&root).expect("private root");
		fs::create_dir(&outside).expect("outside directory");
		fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("private root mode");
		let path = root.join("child");

		let pinned = open_pinned_sandbox_private_root(&path, vec![root.clone()])
			.expect("pinned sandbox root");
		assert_eq!(pinned.path, root);
		assert!(open_pinned_sandbox_private_root(&outside, vec![root.clone()]).is_err());

		fs::set_permissions(&root, fs::Permissions::from_mode(0o1700)).expect("sticky root mode");
		assert!(open_pinned_sandbox_private_root(&path, vec![root.clone()]).is_err());

		fs::remove_dir(&root).expect("remove unsafe root");
		symlink(&outside, &root).expect("replace root with symlink");
		assert!(open_pinned_sandbox_private_root(&path, vec![root]).is_err());
	}

	#[test]
	fn sandbox_read_root_allows_safe_read_only_candidate_modes() {
		let temp = tempfile::tempdir().expect("temporary directory");
		let root = temp.path().join("candidate");
		fs::create_dir(&root).expect("candidate root");
		fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).expect("candidate mode");
		let pinned = open_pinned_sandbox_read_root(root.clone()).expect("pinned read root");
		assert_eq!(pinned.path, root);

		fs::set_permissions(&root, fs::Permissions::from_mode(0o775))
			.expect("unsafe candidate mode");
		assert!(open_pinned_sandbox_read_root(root).is_err());
	}

	#[test]
	fn descriptor_relative_publish_stays_in_the_opened_directory_after_path_replacement() {
		let temp = tempfile::tempdir().expect("temporary directory");
		let original = temp.path().join("state");
		let retained = temp.path().join("retained");
		let outside = temp.path().join("outside");
		fs::create_dir(&original).expect("original directory");
		fs::create_dir(&outside).expect("outside directory");
		let parent = open_private_directory(&original, false).expect("pinned directory");
		fs::rename(&original, &retained).expect("move pinned directory");
		symlink(&outside, &original).expect("replace pathname with symlink");
		let logical_path = original.join("record.json");

		write_new_json_in_parent(
			&parent,
			"record.json".as_ref(),
			&logical_path,
			&json!({"ok": true}),
		)
		.expect("descriptor-relative write");

		assert!(retained.join("record.json").is_file());
		assert!(!outside.join("record.json").exists());
	}
}
