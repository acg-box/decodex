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
	sync::{Arc, OnceLock},
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

#[derive(Clone)]
pub(crate) struct PinnedPrivateDirectory(Arc<PrivateDirectory>);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct PrivateFileIdentity {
	dev: u64,
	ino: u64,
	len: u64,
	mtime: i64,
	mtime_nsec: i64,
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
			candidate.join("automations/decodex/skills/x-post-publisher/SKILL.md").is_file()
				&& candidate.join("apps/decodex-publisher/src/lib.rs").is_file()
		})
		.map(Path::to_path_buf)
		.ok_or_else(|| eyre::eyre!("could not locate repository root"))?;
	let root = clean_absolute_path(&root)?;
	let _ = ROOT.set(root.clone());

	Ok(root)
}

pub(crate) fn resolve_against(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() { path.to_path_buf() } else { root.join(path) }
}

pub(crate) fn path_arg(root: &Path, path: &Path) -> String {
	path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

pub(crate) fn load_json(path: &Path) -> Result<Value> {
	let mut file = open_private_file(path)?;
	read_json(&mut file, path)
}

pub(crate) fn load_json_with_sha256(path: &Path) -> Result<(Value, String)> {
	load_json_with_sha256_bounded(path, MAX_PRIVATE_JSON_BYTES)
}

pub(crate) fn load_json_bytes_with_sha256(path: &Path) -> Result<(Value, Vec<u8>, String)> {
	let mut file = open_private_file(path)?;
	let (value, bytes) = read_json_bytes_bounded(&mut file, path, MAX_PRIVATE_JSON_BYTES)?;
	let digest = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();

	Ok((value, bytes, digest))
}

pub(crate) fn load_json_with_sha256_bounded(
	path: &Path,
	max_bytes: u64,
) -> Result<(Value, String)> {
	let mut file = open_private_file(path)?;
	let (value, bytes) = read_json_bytes_bounded(&mut file, path, max_bytes)?;
	let digest = Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect();

	Ok((value, digest))
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

pub(crate) fn open_existing_exact_private_directory(
	path: &Path,
) -> Result<Option<PinnedPrivateDirectory>> {
	let directory = match open_private_directory(path, false) {
		Ok(directory) => directory,
		Err(error) if error.downcast_ref::<Errno>() == Some(&Errno::NOENT) => return Ok(None),
		Err(error) => return Err(error),
	};
	validate_exact_private_directory(&directory.file)?;

	Ok(Some(PinnedPrivateDirectory(Arc::new(directory))))
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

impl PinnedPrivateDirectory {
	pub(crate) fn identity(&self) -> Result<(u64, u64)> {
		let metadata = self.0.file.metadata()?;
		validate_exact_private_directory_metadata(&metadata)?;

		Ok((metadata.dev(), metadata.ino()))
	}

	pub(crate) fn verify_current_path(&self) -> Result<()> {
		let current = open_private_directory(&self.0.path, false)?;
		validate_exact_private_directory(&current.file)?;
		let expected = self.0.file.metadata()?;
		let actual = current.file.metadata()?;
		if actual.dev() != expected.dev() || actual.ino() != expected.ino() {
			return Err(eyre::eyre!("private directory changed after scan"));
		}

		Ok(())
	}

	pub(crate) fn entries_bounded(&self, max_entries: usize) -> Result<Vec<OsString>> {
		let mut entries = Vec::new();
		for entry in Dir::read_from(&self.0.file)? {
			let entry = entry?;
			let name = OsString::from_vec(entry.file_name().to_bytes().to_vec());
			if name != "." && name != ".." {
				if entries.len() >= max_entries {
					return Err(eyre::eyre!("private directory exceeds its bounded entry limit"));
				}
				entries.push(name);
			}
		}
		entries.sort();

		Ok(entries)
	}

	pub(crate) fn open_child_directory(&self, name: &OsStr) -> Result<Self> {
		validate_child_name(name)?;
		let fd = unix_fs::openat(
			&self.0.file,
			name,
			OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
			Mode::empty(),
		)?;
		let file = File::from(fd);
		validate_exact_private_directory(&file)?;

		Ok(Self(Arc::new(PrivateDirectory {
			file,
			path: self.0.path.join(name),
			private_anchor_found: true,
		})))
	}

	pub(crate) fn read_json(
		&self,
		name: &OsStr,
		max_bytes: u64,
	) -> Result<(Value, PrivateFileIdentity, Vec<u8>)> {
		validate_child_name(name)?;
		let path = self.0.path.join(name);
		let mut file = open_named_private_file(&self.0, name, &path)?;
		let metadata = file.metadata()?;
		if metadata.len() > max_bytes {
			return Err(eyre::eyre!("private JSON exceeds its bounded read limit"));
		}
		let identity = PrivateFileIdentity::from_metadata(&metadata);
		let (value, bytes) = read_json_bytes_bounded(&mut file, &path, max_bytes)?;

		Ok((value, identity, bytes))
	}

	pub(crate) fn verify_file(&self, name: &OsStr, expected: PrivateFileIdentity) -> Result<()> {
		validate_child_name(name)?;
		let path = self.0.path.join(name);
		let file = open_named_private_file(&self.0, name, &path)?;
		let metadata = file.metadata()?;
		if PrivateFileIdentity::from_metadata(&metadata) != expected {
			return Err(eyre::eyre!("private JSON changed after scan"));
		}

		Ok(())
	}

	pub(crate) fn unlink_verified(
		&self,
		name: &OsStr,
		expected: PrivateFileIdentity,
	) -> Result<()> {
		self.verify_file(name, expected)?;
		unix_fs::unlinkat(&self.0.file, name, AtFlags::empty())?;

		Ok(())
	}

	pub(crate) fn sync(&self) -> Result<()> {
		self.0.file.sync_all()?;

		Ok(())
	}
}

impl PrivateFileIdentity {
	fn from_metadata(metadata: &fs::Metadata) -> Self {
		Self {
			dev: metadata.dev(),
			ino: metadata.ino(),
			len: metadata.len(),
			mtime: metadata.mtime(),
			mtime_nsec: metadata.mtime_nsec(),
		}
	}

	pub(crate) fn len(self) -> u64 {
		self.len
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
		OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
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
			OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
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
	let fd = unix_fs::openat(
		&parent.file,
		file_name,
		OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
		Mode::empty(),
	)
	.map_err(|error| {
		eyre::eyre!("private JSON must be a regular non-symlink file {}: {error}", path.display())
	})?;
	let file = File::from(fd);
	validate_private_json_metadata(path, &file.metadata()?)?;

	Ok(file)
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

fn open_private_directory(path: &Path, create: bool) -> Result<PrivateDirectory> {
	let path = clean_absolute_path(path)?;
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

	for component in path.components() {
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

fn validate_exact_private_directory(file: &File) -> Result<()> {
	validate_exact_private_directory_metadata(&file.metadata()?)
}

fn validate_exact_private_directory_metadata(metadata: &fs::Metadata) -> Result<()> {
	if !metadata.is_dir()
		|| metadata.uid() != current_uid()
		|| metadata.permissions().mode() & 0o777 != u32::from(PRIVATE_DIRECTORY_MODE)
	{
		return Err(eyre::eyre!("private directory is not an owned mode-0700 directory"));
	}

	Ok(())
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

fn validate_child_name(name: &OsStr) -> Result<()> {
	let path = Path::new(name);
	if name.is_empty()
		|| path.components().count() != 1
		|| !matches!(path.components().next(), Some(Component::Normal(_)))
	{
		return Err(eyre::eyre!("private child name is invalid"));
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
	use std::{fs, os::unix::fs::symlink};

	use serde_json::json;

	use super::{open_private_directory, write_new_json_in_parent};

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
