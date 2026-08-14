#[cfg(not(any(unix, windows)))] use std::ffi::OsString;
#[cfg(unix)] use std::fs::File;
use std::{
	env,
	ffi::OsStr,
	fmt::{Debug, Display, Formatter},
	fs::Metadata,
	io,
	path::{Component, Path, PathBuf},
};
#[cfg(not(unix))] use std::{
	fs::{self, DirBuilder, OpenOptions},
	io::{Read, Write},
};

use crate::path_unix;

pub(crate) const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
pub(crate) const PRIVATE_FILE_MODE: u32 = 0o600;
pub(crate) const ATOMIC_TEMPORARY_PREFIX: &str = ".tmp-";

const MAX_ROOT_PATH_BYTES: usize = 4 * 1_024;

/// Typed Decodex-owned root. All product-owned paths are derived from this value.
#[derive(Clone, Eq, PartialEq)]
pub struct DecodexRoot(PathBuf);
impl DecodexRoot {
	/// Validate an explicitly configured Decodex root.
	///
	/// The root must be absolute, distinct from the filesystem root, and outside every
	/// `.codex` subtree. Accepted lexical `.` and repeated-separator forms are stored in
	/// one normalized representation; parent traversal is rejected. The `.codex` guard
	/// prevents vNext product state from entering Codex-owned storage.
	pub fn new(path: impl Into<PathBuf>) -> Result<Self, PathError> {
		let path = path.into();
		let encoded = path.as_os_str().as_encoded_bytes();

		if encoded.len() > MAX_ROOT_PATH_BYTES || encoded.contains(&0) {
			return Err(PathError::UnsafeRoot);
		}

		let path = normalize_absolute(path)?;

		for component in path.components() {
			match component {
				Component::ParentDir | Component::CurDir => return Err(PathError::UnsafeRoot),
				Component::Normal(value) if is_codex_component(value) => {
					return Err(PathError::CodexOwnedRoot);
				},
				_ => {},
			}
		}

		Ok(Self(path))
	}

	/// Derive the canonical Decodex root below an explicit platform home directory.
	pub fn from_home(home: impl AsRef<Path>) -> Result<Self, PathError> {
		let home = home.as_ref();

		if !home.is_absolute() || home.parent().is_none() {
			return Err(PathError::UnsafeRoot);
		}

		Self::new(home.join(".decodex"))
	}

	/// Resolve the platform home and derive its canonical `~/.decodex` root.
	pub fn platform_default() -> Result<Self, PathError> {
		#[cfg(unix)]
		let home = env::var_os("HOME");
		#[cfg(windows)]
		let home = env::var_os("USERPROFILE");
		#[cfg(not(any(unix, windows)))]
		let home: Option<OsString> = None;
		let home = home.filter(|value| !value.is_empty()).ok_or(PathError::HomeUnavailable)?;

		Self::from_home(PathBuf::from(home))
	}

	/// Absolute root path.
	pub fn as_path(&self) -> &Path {
		&self.0
	}

	/// Derive the complete owned path layout.
	pub fn paths(&self) -> DecodexPaths {
		DecodexPaths { root: self.clone() }
	}
}

impl Debug for DecodexRoot {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("DecodexRoot(<redacted>)")
	}
}

/// Fixed, typed path layout for Decodex-owned vNext files.
#[derive(Clone, Eq, PartialEq)]
pub struct DecodexPaths {
	root: DecodexRoot,
}
impl DecodexPaths {
	/// Configured Decodex-owned root.
	pub fn root(&self) -> &DecodexRoot {
		&self.root
	}

	/// Bounded operator configuration file.
	pub fn config_file(&self) -> PathBuf {
		self.join("config.toml")
	}

	/// Decodex-owned log directory.
	pub fn logs_dir(&self) -> PathBuf {
		self.join("logs")
	}

	/// Content-addressed SHA-256 blob directory.
	pub fn blobs_dir(&self) -> PathBuf {
		self.join("blobs/sha256")
	}

	/// Disposable bounded cache directory.
	pub fn cache_dir(&self) -> PathBuf {
		self.join("cache")
	}

	/// Server-host-only state directory.
	pub fn server_dir(&self) -> PathBuf {
		self.join("server")
	}

	/// Stable server identity file.
	pub fn server_identity_file(&self) -> PathBuf {
		self.join("server/identity")
	}

	/// Fixed retired credential-vault path used only by the one-shot transfer tool.
	pub fn credential_vault_file(&self) -> PathBuf {
		self.join("server/credentials.redb")
	}

	/// Daemon-owned local product database.
	pub fn product_database_file(&self) -> PathBuf {
		self.join("server/decodex.sqlite3")
	}

	/// Create and verify the owner-private local product database file.
	#[cfg(unix)]
	pub fn open_product_database_file(&self) -> Result<File, PathError> {
		self.ensure_owned_directory(Path::new("server"))?;
		path_unix::open_private_database_file(self, &self.product_database_file())
	}

	/// Owner-only external ProcessGeneration execution authorization.
	pub fn process_execution_authorization_file(&self) -> PathBuf {
		self.join("server/process-execution-authorization")
	}

	/// Fixed owner-only local product endpoint.
	pub fn local_transport_socket(&self) -> PathBuf {
		self.join("server/decodex.sock")
	}

	/// Create and verify only the root and server directory required by local transport.
	///
	/// This does not create a server identity or any database, blob, cache, or log state.
	pub fn ensure_local_transport_layout(&self) -> Result<(), PathError> {
		#[cfg(unix)]
		{
			path_unix::ensure_owned_directory(self, Path::new("server"))?;

			Ok(())
		}

		#[cfg(not(unix))]
		{
			ensure_private_directory(self.root.as_path())?;
			ensure_private_directory(&self.server_dir())
		}
	}

	/// Create and verify the private fixed directory layout.
	pub fn ensure_layout(&self) -> Result<(), PathError> {
		#[cfg(unix)]
		{
			path_unix::ensure_layout(self)
		}

		#[cfg(not(unix))]
		{
			verify_existing_ancestors(self.root.as_path())?;
			ensure_private_directory(self.root.as_path())?;

			for relative in ["logs", "blobs", "blobs/sha256", "cache", "server"] {
				self.ensure_owned_directory(Path::new(relative))?;
			}

			Ok(())
		}
	}

	pub(crate) fn join(&self, relative: impl AsRef<Path>) -> PathBuf {
		self.root.as_path().join(relative)
	}

	pub(crate) fn ensure_owned_directory(&self, relative: &Path) -> Result<PathBuf, PathError> {
		#[cfg(unix)]
		{
			path_unix::ensure_owned_directory(self, relative)
		}

		#[cfg(not(unix))]
		{
			validate_relative(relative)?;
			verify_existing_ancestors(self.root.as_path())?;
			ensure_private_directory(self.root.as_path())?;

			let mut current = self.root.as_path().to_path_buf();

			for component in relative.components() {
				let Component::Normal(component) = component else {
					return Err(PathError::Escape);
				};

				current.push(component);

				ensure_private_directory(&current)?;
			}

			Ok(current)
		}
	}

	#[cfg(not(unix))]
	pub(crate) fn validate_file_parent(&self, path: &Path) -> Result<(), PathError> {
		let parent = path.parent().ok_or(PathError::Escape)?;
		let relative = parent.strip_prefix(self.root.as_path()).map_err(|_| PathError::Escape)?;

		validate_relative(relative)?;
		verify_existing_ancestors(self.root.as_path())?;
		verify_private_directory(self.root.as_path())?;

		let mut current = self.root.as_path().to_path_buf();

		for component in relative.components() {
			let Component::Normal(component) = component else {
				return Err(PathError::Escape);
			};

			current.push(component);

			verify_private_directory(&current)?;
		}

		Ok(())
	}
}

impl Debug for DecodexPaths {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("DecodexPaths(<redacted>)")
	}
}

/// Typed fail-closed filesystem error. It intentionally stores no file contents or
/// underlying error strings, so malformed secret-bearing input cannot leak through
/// `Display` or `Debug`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathError {
	/// Root was relative, used parent traversal, or was the filesystem root.
	UnsafeRoot,
	/// Root was placed below Codex-owned `.codex` storage.
	CodexOwnedRoot,
	/// The platform home directory was unavailable.
	HomeUnavailable,
	/// A caller-supplied path escaped its typed owner.
	Escape,
	/// A symbolic link appeared at an owned boundary.
	Symlink,
	/// A directory was expected but another file kind was present.
	UnexpectedDirectoryKind,
	/// A regular file was expected but another file kind was present.
	UnexpectedFileKind,
	/// Group/other access, executable file bits, or missing owner access was present.
	InsecurePermissions,
	/// An input exceeded its explicit byte limit.
	Oversized {
		/// Maximum accepted bytes.
		limit: usize,
	},
	/// A create-only atomic target already exists.
	AlreadyExists,
	/// The operating system randomness source was unavailable.
	RandomnessUnavailable,
	/// A bounded I/O operation failed without retaining potentially sensitive details.
	Io {
		/// Stable operation category.
		operation: IoOperation,
		/// Redacted standard-library error category.
		kind: io::ErrorKind,
	},
}
impl Display for PathError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::UnsafeRoot => formatter.write_str("unsafe Decodex root"),
			Self::CodexOwnedRoot => formatter.write_str("Decodex root cannot be inside .codex"),
			Self::HomeUnavailable => formatter.write_str("platform home directory is unavailable"),
			Self::Escape => formatter.write_str("path escapes its Decodex owner"),
			Self::Symlink => formatter.write_str("symbolic links are forbidden in Decodex storage"),
			Self::UnexpectedDirectoryKind => formatter.write_str("expected a private directory"),
			Self::UnexpectedFileKind => formatter.write_str("expected a private regular file"),
			Self::InsecurePermissions => formatter.write_str("insecure Decodex file permissions"),
			Self::Oversized { limit } => write!(formatter, "input exceeds the {limit}-byte limit"),
			Self::AlreadyExists => formatter.write_str("atomic target already exists"),
			Self::RandomnessUnavailable =>
				formatter.write_str("operating system randomness is unavailable"),
			Self::Io { operation, kind } => write!(formatter, "{operation:?} failed: {kind:?}"),
		}
	}
}

impl std::error::Error for PathError {}

/// Stable operation labels for redacted filesystem errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IoOperation {
	CreateDirectory,
	Inspect,
	Open,
	Read,
	Write,
	Sync,
	Link,
	Rename,
	Remove,
	List,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum AtomicMode {
	CreateOnly,
	Replace,
}

pub(crate) fn is_atomic_temporary_file(path: &Path) -> bool {
	let Some(name) = path.file_name().and_then(OsStr::to_str) else { return false };
	let Some(random) = name.strip_prefix(ATOMIC_TEMPORARY_PREFIX) else { return false };

	random.len() == 32
		&& random.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(crate) fn read_private_file(
	paths: &DecodexPaths,
	path: &Path,
	maximum_bytes: usize,
) -> Result<Vec<u8>, PathError> {
	#[cfg(unix)]
	{
		path_unix::read_private_file(paths, path, maximum_bytes)
	}

	#[cfg(not(unix))]
	paths.validate_file_parent(path)?;

	#[cfg(not(unix))]
	{
		let metadata = private_file_metadata(path)?;

		if metadata.len() > u64::try_from(maximum_bytes).unwrap_or(u64::MAX) {
			return Err(PathError::Oversized { limit: maximum_bytes });
		}

		let mut options = OpenOptions::new();

		options.read(true);

		apply_no_follow(&mut options);

		let file = options.open(path).map_err(|error| io_error(IoOperation::Open, error))?;

		verify_private_file_metadata(
			&file.metadata().map_err(|error| io_error(IoOperation::Inspect, error))?,
		)?;

		let mut bytes = Vec::with_capacity(metadata.len().min(maximum_bytes as u64) as usize);

		file.take((maximum_bytes as u64).saturating_add(1))
			.read_to_end(&mut bytes)
			.map_err(|error| io_error(IoOperation::Read, error))?;

		if bytes.len() > maximum_bytes {
			return Err(PathError::Oversized { limit: maximum_bytes });
		}

		Ok(bytes)
	}
}

pub(crate) fn atomic_write_new(
	paths: &DecodexPaths,
	path: &Path,
	bytes: &[u8],
	maximum_bytes: usize,
) -> Result<(), PathError> {
	#[cfg(unix)]
	{
		path_unix::atomic_write(paths, path, bytes, maximum_bytes, AtomicMode::CreateOnly)
	}
	#[cfg(not(unix))]
	{
		atomic_write(paths, path, bytes, maximum_bytes, AtomicMode::CreateOnly)
	}
}

pub(crate) fn atomic_write_replace(
	paths: &DecodexPaths,
	path: &Path,
	bytes: &[u8],
	maximum_bytes: usize,
) -> Result<(), PathError> {
	#[cfg(unix)]
	{
		path_unix::atomic_write(paths, path, bytes, maximum_bytes, AtomicMode::Replace)
	}
	#[cfg(not(unix))]
	{
		atomic_write(paths, path, bytes, maximum_bytes, AtomicMode::Replace)
	}
}

pub(crate) fn remove_private_file(paths: &DecodexPaths, path: &Path) -> Result<(), PathError> {
	#[cfg(unix)]
	{
		path_unix::remove_private_file(paths, path)
	}

	#[cfg(not(unix))]
	{
		paths.validate_file_parent(path)?;

		private_file_metadata(path)?;

		fs::remove_file(path).map_err(|error| io_error(IoOperation::Remove, error))?;

		sync_directory(path.parent().ok_or(PathError::Escape)?)
	}
}

pub(crate) fn visit_private_files<E>(
	paths: &DecodexPaths,
	directory: &Path,
	visitor: impl FnMut(PathBuf, Metadata) -> Result<(), E>,
) -> Result<(), E>
where
	E: From<PathError>,
{
	#[cfg(unix)]
	{
		path_unix::visit_private_files(paths, directory, visitor)
	}

	#[cfg(not(unix))]
	{
		let relative =
			directory.strip_prefix(paths.root.as_path()).map_err(|_| PathError::Escape.into())?;

		validate_relative(relative).map_err(E::from)?;
		verify_existing_ancestors(paths.root.as_path()).map_err(E::from)?;
		verify_private_directory(directory).map_err(E::from)?;

		let entries =
			fs::read_dir(directory).map_err(|error| E::from(io_error(IoOperation::List, error)))?;
		let mut visitor = visitor;

		for entry in entries {
			let entry = entry.map_err(|error| E::from(io_error(IoOperation::List, error)))?;
			let path = entry.path();
			let metadata = fs::symlink_metadata(&path)
				.map_err(|error| E::from(io_error(IoOperation::Inspect, error)))?;
			let file_type = entry
				.file_type()
				.map_err(|error| E::from(io_error(IoOperation::Inspect, error)))?;

			if file_type.is_symlink() {
				return Err(PathError::Symlink.into());
			}
			if !file_type.is_file() {
				return Err(PathError::UnexpectedFileKind.into());
			}

			verify_private_file_metadata(&metadata).map_err(E::from)?;
			visitor(path, metadata)?;
		}

		Ok(())
	}
}

pub(crate) fn validate_relative(path: &Path) -> Result<(), PathError> {
	if path.is_absolute() {
		return Err(PathError::Escape);
	}

	for component in path.components() {
		if !matches!(component, Component::Normal(_)) {
			return Err(PathError::Escape);
		}
	}

	Ok(())
}

#[cfg(not(unix))]
pub(crate) fn verify_directory_permissions(_metadata: &Metadata) -> Result<(), PathError> {
	Ok(())
}

#[cfg(not(unix))]
pub(crate) fn verify_file_permissions(_metadata: &Metadata) -> Result<(), PathError> {
	Ok(())
}

pub(crate) fn io_error(operation: IoOperation, error: io::Error) -> PathError {
	PathError::Io { operation, kind: error.kind() }
}

#[cfg(not(unix))]
fn atomic_write(
	paths: &DecodexPaths,
	path: &Path,
	bytes: &[u8],
	maximum_bytes: usize,
	mode: AtomicMode,
) -> Result<(), PathError> {
	if bytes.len() > maximum_bytes {
		return Err(PathError::Oversized { limit: maximum_bytes });
	}

	paths.validate_file_parent(path)?;

	match fs::symlink_metadata(path) {
		Ok(metadata) => {
			if metadata.file_type().is_symlink() {
				return Err(PathError::Symlink);
			}

			verify_private_file_metadata(&metadata)?;

			if mode == AtomicMode::CreateOnly {
				return Err(PathError::AlreadyExists);
			}
		},
		Err(error) if error.kind() == io::ErrorKind::NotFound => {},
		Err(error) => return Err(io_error(IoOperation::Inspect, error)),
	}

	let parent = path.parent().ok_or(PathError::Escape)?;
	let temporary = unique_temporary_path(parent)?;
	let result = (|| {
		let mut options = OpenOptions::new();

		options.write(true).create_new(true);

		apply_private_create(&mut options);

		let mut file =
			options.open(&temporary).map_err(|error| io_error(IoOperation::Open, error))?;

		file.write_all(bytes).map_err(|error| io_error(IoOperation::Write, error))?;
		file.sync_all().map_err(|error| io_error(IoOperation::Sync, error))?;

		verify_private_file_metadata(
			&file.metadata().map_err(|error| io_error(IoOperation::Inspect, error))?,
		)?;
		drop(file);

		match mode {
			AtomicMode::Replace => fs::rename(&temporary, path)
				.map_err(|error| io_error(IoOperation::Rename, error))?,
			AtomicMode::CreateOnly => match fs::hard_link(&temporary, path) {
				Ok(()) => fs::remove_file(&temporary)
					.map_err(|error| io_error(IoOperation::Remove, error))?,
				Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
					return Err(PathError::AlreadyExists);
				},
				Err(error) => return Err(io_error(IoOperation::Link, error)),
			},
		}

		sync_directory(parent)
	})();

	if temporary.exists() {
		let _ = fs::remove_file(&temporary);
	}

	result
}

#[cfg(not(unix))]
fn unique_temporary_path(parent: &Path) -> Result<PathBuf, PathError> {
	for _ in 0..8 {
		let mut random = [0_u8; 16];

		getrandom::fill(&mut random).map_err(|_| PathError::RandomnessUnavailable)?;

		let name = format!("{ATOMIC_TEMPORARY_PREFIX}{}", hex(&random));
		let candidate = parent.join(name);

		if !candidate.exists() {
			return Ok(candidate);
		}
	}

	Err(PathError::AlreadyExists)
}

fn normalize_absolute(path: PathBuf) -> Result<PathBuf, PathError> {
	if !path.is_absolute() || path.parent().is_none() {
		return Err(PathError::UnsafeRoot);
	}

	let mut normalized = PathBuf::new();

	for component in path.components() {
		match component {
			Component::ParentDir => return Err(PathError::UnsafeRoot),
			Component::CurDir => {},
			Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
				normalized.push(component.as_os_str());
			},
		}
	}

	if normalized.parent().is_none() {
		return Err(PathError::UnsafeRoot);
	}

	Ok(normalized)
}

#[cfg(not(unix))]
fn verify_existing_ancestors(path: &Path) -> Result<(), PathError> {
	let parent = path.parent().ok_or(PathError::UnsafeRoot)?;
	let mut current = PathBuf::new();

	for component in parent.components() {
		current.push(component.as_os_str());

		match fs::symlink_metadata(&current) {
			Ok(metadata) => {
				if metadata.file_type().is_symlink() {
					return Err(PathError::Symlink);
				}
				if !metadata.is_dir() {
					return Err(PathError::UnexpectedDirectoryKind);
				}
			},
			Err(error) if error.kind() == io::ErrorKind::NotFound => break,
			Err(error) => return Err(io_error(IoOperation::Inspect, error)),
		}
	}

	Ok(())
}

fn is_codex_component(value: &OsStr) -> bool {
	value.to_string_lossy().eq_ignore_ascii_case(".codex")
}

#[cfg(not(unix))]
fn ensure_private_directory(path: &Path) -> Result<(), PathError> {
	match fs::symlink_metadata(path) {
		Ok(metadata) => verify_private_directory_metadata(&metadata),
		Err(error) if error.kind() == io::ErrorKind::NotFound => {
			let mut builder = DirBuilder::new();

			#[cfg(unix)]
			builder.mode(PRIVATE_DIRECTORY_MODE);

			match builder.create(path) {
				Ok(()) => {},
				Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {},
				Err(error) => return Err(io_error(IoOperation::CreateDirectory, error)),
			}

			verify_private_directory(path)
		},
		Err(error) => Err(io_error(IoOperation::Inspect, error)),
	}
}

#[cfg(not(unix))]
fn verify_private_directory(path: &Path) -> Result<(), PathError> {
	let metadata =
		fs::symlink_metadata(path).map_err(|error| io_error(IoOperation::Inspect, error))?;

	verify_private_directory_metadata(&metadata)
}

#[cfg(not(unix))]
fn verify_private_directory_metadata(metadata: &Metadata) -> Result<(), PathError> {
	if metadata.file_type().is_symlink() {
		return Err(PathError::Symlink);
	}
	if !metadata.is_dir() {
		return Err(PathError::UnexpectedDirectoryKind);
	}

	verify_directory_permissions(metadata)
}

#[cfg(not(unix))]
fn private_file_metadata(path: &Path) -> Result<Metadata, PathError> {
	let metadata =
		fs::symlink_metadata(path).map_err(|error| io_error(IoOperation::Inspect, error))?;

	if metadata.file_type().is_symlink() {
		return Err(PathError::Symlink);
	}

	verify_private_file_metadata(&metadata)?;

	Ok(metadata)
}

#[cfg(not(unix))]
fn verify_private_file_metadata(metadata: &Metadata) -> Result<(), PathError> {
	if !metadata.is_file() {
		return Err(PathError::UnexpectedFileKind);
	}

	verify_file_permissions(metadata)
}

#[cfg(not(unix))]
fn sync_directory(path: &Path) -> Result<(), PathError> {
	let mut options = OpenOptions::new();

	options.read(true);

	let directory = options.open(path).map_err(|error| io_error(IoOperation::Open, error))?;

	directory.sync_all().map_err(|error| io_error(IoOperation::Sync, error))
}

#[cfg(not(unix))]
fn apply_private_create(_options: &mut OpenOptions) {}

#[cfg(not(unix))]
fn apply_no_follow(_options: &mut OpenOptions) {}

#[cfg(not(unix))]
fn hex(bytes: &[u8]) -> String {
	const HEX: &[u8; 16] = b"0123456789abcdef";

	let mut encoded = String::with_capacity(bytes.len() * 2);

	for &byte in bytes {
		encoded.push(char::from(HEX[usize::from(byte >> 4)]));
		encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
	}

	encoded
}
