//! Exact owner-safe projection of one daemon-held account into Codex shared auth.

use std::{
	ffi::{CStr, CString},
	fs::File,
	io::{self, Read, Write},
	mem::MaybeUninit,
	os::{
		fd::{AsRawFd as _, FromRawFd as _, RawFd},
		unix::{ffi::OsStrExt as _, fs::MetadataExt as _},
	},
	path::{Component, Path, PathBuf},
	sync::{
		Mutex,
		atomic::{AtomicU64, Ordering},
	},
	time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize, de::IgnoredAny};
use sha2::{Digest as _, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::{
	account_import::{ImportedCredential, parse_shared_codex},
	host_credentials::CredentialSecretBundle,
};

const AUTH_FILE_NAME: &CStr = c"auth.json";
const CODEX_DIRECTORY_NAME: &CStr = c".codex";
const MAX_AUTH_FILE_BYTES: usize = 256 * 1024;
const TEMP_CREATE_ATTEMPTS: u64 = 8;

static PROJECTION_LOCK: Mutex<()> = Mutex::new(());
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Credential-negative metadata used to coalesce stable shared-auth reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SharedCodexAuthFileStamp {
	Absent,
	Present {
		device: u64,
		inode: u64,
		length: u64,
		modified_seconds: i64,
		modified_nanoseconds: i64,
	},
}

/// Exact credential-negative source version required by the final Route CAS.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SharedCodexAuthVersion {
	pub(crate) stamp: SharedCodexAuthFileStamp,
	pub(crate) sha256: Option<[u8; 32]>,
}

/// One stable, bounded read of the normal shared Codex auth source.
pub(crate) enum SharedCodexAuthSnapshot {
	Managed { version: SharedCodexAuthVersion, credential: ImportedCredential },
	Unmanaged { version: SharedCodexAuthVersion },
}

impl SharedCodexAuthSnapshot {
	pub(crate) const fn version(&self) -> &SharedCodexAuthVersion {
		match self {
			Self::Managed { version, .. } | Self::Unmanaged { version } => version,
		}
	}
}

/// Read only the exact metadata needed by the daemon's stable-read coalescer.
pub(crate) fn read_shared_codex_auth_stamp()
-> Result<SharedCodexAuthFileStamp, CodexAuthProjectionError> {
	let home = codex_home()?;
	let _guard = PROJECTION_LOCK.lock().map_err(|_| CodexAuthProjectionError::Unavailable)?;
	let directory = open_codex_directory(&home)?;
	let identity = directory_identity(&directory)?;
	let stamp = read_file_stamp(&directory)?;
	let current = open_codex_directory(&home)?;
	if directory_identity(&current)? != identity {
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	Ok(stamp)
}

/// Read one source only while its metadata remains equal to two prior poll observations.
pub(crate) fn read_shared_codex_auth_snapshot(
	expected: &SharedCodexAuthFileStamp,
) -> Result<SharedCodexAuthSnapshot, CodexAuthProjectionError> {
	let home = codex_home()?;
	let _guard = PROJECTION_LOCK.lock().map_err(|_| CodexAuthProjectionError::Unavailable)?;
	let directory = open_codex_directory(&home)?;
	let identity = directory_identity(&directory)?;
	let snapshot = read_snapshot_from_directory(&directory, expected)?;
	let current = open_codex_directory(&home)?;
	if directory_identity(&current)? != identity {
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	Ok(snapshot)
}

/// Project one exact target only while the complete stable source version remains current.
pub(crate) fn project_shared_codex_auth_cas(
	bundle: &CredentialSecretBundle,
	provider_account_id: &str,
	expected_source: &SharedCodexAuthVersion,
) -> Result<(), CodexAuthProjectionError> {
	let home = codex_home()?;
	project_shared_codex_auth_cas_at(
		&home,
		bundle,
		provider_account_id,
		expected_source,
		ProjectionFault::None,
	)
}

fn project_shared_codex_auth_cas_at(
	home: &Path,
	bundle: &CredentialSecretBundle,
	provider_account_id: &str,
	expected_source: &SharedCodexAuthVersion,
	fault: ProjectionFault,
) -> Result<(), CodexAuthProjectionError> {
	let _guard = PROJECTION_LOCK.lock().map_err(|_| CodexAuthProjectionError::Unavailable)?;
	let directory = open_codex_directory(home)?;
	let identity = directory_identity(&directory)?;
	if read_file_version(&directory)? != *expected_source {
		return Err(CodexAuthProjectionError::SourceChanged);
	}
	let mutation = project_to_directory_inner(
		&directory,
		bundle,
		provider_account_id,
		None,
		Some(expected_source),
		fault,
	)?;
	let current =
		open_codex_directory(home).map_err(|error| after_projection_error(mutation, error))?;
	let current_identity =
		directory_identity(&current).map_err(|error| after_projection_error(mutation, error))?;
	if current_identity != identity {
		return Err(after_projection_error(mutation, CodexAuthProjectionError::UnsafePath));
	}
	Ok(())
}

#[cfg(test)]
fn project_shared_codex_auth_at(
	home: &Path,
	bundle: &CredentialSecretBundle,
	provider_account_id: &str,
	fault: ProjectionFault,
) -> Result<(), CodexAuthProjectionError> {
	project_shared_codex_auth_with_precondition_at(home, bundle, provider_account_id, None, fault)
}

#[cfg(test)]
fn project_shared_codex_auth_with_precondition_at(
	home: &Path,
	bundle: &CredentialSecretBundle,
	provider_account_id: &str,
	expected_source: Option<(&CredentialSecretBundle, &str)>,
	fault: ProjectionFault,
) -> Result<(), CodexAuthProjectionError> {
	let _guard = PROJECTION_LOCK.lock().map_err(|_| CodexAuthProjectionError::Unavailable)?;
	let directory = open_codex_directory(home)?;
	let identity = directory_identity(&directory)?;
	let expected_target = if let Some((expected_bundle, expected_provider_account_id)) =
		expected_source
	{
		let before = inspect_target(&directory)?;
		let id_token = bundle.id_token().ok_or(CodexAuthProjectionError::MissingIdentityToken)?;
		let target_is_current =
			current_auth_matches(&directory, bundle, id_token, provider_account_id)?;
		let source_is_current = if target_is_current {
			true
		} else {
			let expected_id_token =
				expected_bundle.id_token().ok_or(CodexAuthProjectionError::MissingIdentityToken)?;
			current_auth_matches(
				&directory,
				expected_bundle,
				expected_id_token,
				expected_provider_account_id,
			)?
		};
		if !source_is_current || inspect_target(&directory)? != before {
			return Err(CodexAuthProjectionError::SourceChanged);
		}
		Some(before)
	} else {
		None
	};
	let mutation = project_to_directory_inner(
		&directory,
		bundle,
		provider_account_id,
		expected_target,
		None,
		fault,
	)?;
	#[cfg(test)]
	if fault == ProjectionFault::AfterRenamePathRevalidation {
		return Err(after_projection_error(mutation, CodexAuthProjectionError::Unavailable));
	}
	let current =
		open_codex_directory(home).map_err(|error| after_projection_error(mutation, error))?;
	let current_identity =
		directory_identity(&current).map_err(|error| after_projection_error(mutation, error))?;
	if current_identity != identity {
		return Err(after_projection_error(mutation, CodexAuthProjectionError::UnsafePath));
	}

	Ok(())
}

fn codex_home() -> Result<PathBuf, CodexAuthProjectionError> {
	if std::env::var_os("CODEX_HOME").is_some_and(|value| !value.is_empty()) {
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	std::env::var_os("HOME")
		.filter(|value| !value.is_empty())
		.map(PathBuf::from)
		.ok_or(CodexAuthProjectionError::UnsafePath)
}

#[cfg(test)]
fn project_to_directory(
	directory: &File,
	bundle: &CredentialSecretBundle,
	provider_account_id: &str,
) -> Result<(), CodexAuthProjectionError> {
	project_to_directory_inner(
		directory,
		bundle,
		provider_account_id,
		None,
		None,
		ProjectionFault::None,
	)
	.map(|_| ())
}

fn project_to_directory_inner(
	directory: &File,
	bundle: &CredentialSecretBundle,
	provider_account_id: &str,
	expected_target: Option<Option<TargetIdentity>>,
	expected_version: Option<&SharedCodexAuthVersion>,
	fault: ProjectionFault,
) -> Result<ProjectionMutation, CodexAuthProjectionError> {
	#[cfg(not(test))]
	let _ = fault;
	if provider_account_id.is_empty()
		|| provider_account_id.len() > 512
		|| provider_account_id.chars().any(char::is_control)
	{
		return Err(CodexAuthProjectionError::InvalidCredential);
	}
	if let Some(expected_version) = expected_version
		&& read_file_version(directory)? != *expected_version
	{
		return Err(CodexAuthProjectionError::SourceChanged);
	}
	let id_token = bundle.id_token().ok_or(CodexAuthProjectionError::MissingIdentityToken)?;
	if current_auth_matches(directory, bundle, id_token, provider_account_id)? {
		return Ok(ProjectionMutation::AlreadyCurrent);
	}
	let encoded = encode_auth(bundle, id_token, provider_account_id)?;
	let original = inspect_target(directory)?;
	if expected_target.is_some_and(|expected| expected != original) {
		return Err(CodexAuthProjectionError::SourceChanged);
	}
	let (mut temporary, temporary_name) = create_temporary(directory)?;
	temporary
		.write_all(&encoded)
		.and_then(|()| temporary.sync_all())
		.map_err(|_| CodexAuthProjectionError::Unavailable)?;
	set_exact_mode(&temporary)?;
	let mut cleanup =
		TemporaryEntry { directory: directory.as_raw_fd(), name: temporary_name, renamed: false };

	if inspect_target(directory)? != original {
		return Err(if expected_target.is_some() {
			CodexAuthProjectionError::SourceChanged
		} else {
			CodexAuthProjectionError::UnsafePath
		});
	}
	if let Some(expected_version) = expected_version
		&& read_file_version(directory)? != *expected_version
	{
		return Err(CodexAuthProjectionError::SourceChanged);
	}
	#[cfg(test)]
	if fault == ProjectionFault::BeforeRename {
		return Err(CodexAuthProjectionError::Unavailable);
	}
	if unsafe {
		libc::renameat(
			directory.as_raw_fd(),
			cleanup.name.as_ptr(),
			directory.as_raw_fd(),
			AUTH_FILE_NAME.as_ptr(),
		)
	} != 0
	{
		return Err(CodexAuthProjectionError::Unavailable);
	}
	cleanup.renamed = true;

	#[cfg(test)]
	if fault == ProjectionFault::AfterRenameOpen {
		return Err(CodexAuthProjectionError::OutcomeUnknown);
	}
	let mut projected =
		open_target(directory).map_err(|_| CodexAuthProjectionError::OutcomeUnknown)?;
	set_exact_mode(&projected).map_err(|_| CodexAuthProjectionError::OutcomeUnknown)?;
	validate_target_file(&projected).map_err(|_| CodexAuthProjectionError::OutcomeUnknown)?;
	#[cfg(test)]
	if fault == ProjectionFault::AfterRenameFileSync {
		return Err(CodexAuthProjectionError::OutcomeUnknown);
	}
	projected.sync_all().map_err(|_| CodexAuthProjectionError::OutcomeUnknown)?;
	#[cfg(test)]
	if fault == ProjectionFault::AfterRenameReadback {
		return Err(CodexAuthProjectionError::OutcomeUnknown);
	}
	readback_exact(&mut projected, &encoded)
		.map_err(|_| CodexAuthProjectionError::OutcomeUnknown)?;
	#[cfg(test)]
	if fault == ProjectionFault::AfterRenameParentSync {
		return Err(CodexAuthProjectionError::OutcomeUnknown);
	}
	directory.sync_all().map_err(|_| CodexAuthProjectionError::OutcomeUnknown)?;

	Ok(ProjectionMutation::Replaced)
}

const fn after_projection_error(
	mutation: ProjectionMutation,
	error: CodexAuthProjectionError,
) -> CodexAuthProjectionError {
	match mutation {
		ProjectionMutation::AlreadyCurrent => error,
		ProjectionMutation::Replaced => CodexAuthProjectionError::OutcomeUnknown,
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProjectionMutation {
	AlreadyCurrent,
	Replaced,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProjectionFault {
	None,
	#[cfg(test)]
	BeforeRename,
	#[cfg(test)]
	AfterRenameOpen,
	#[cfg(test)]
	AfterRenameFileSync,
	#[cfg(test)]
	AfterRenameReadback,
	#[cfg(test)]
	AfterRenameParentSync,
	#[cfg(test)]
	AfterRenamePathRevalidation,
}

fn encode_auth(
	bundle: &CredentialSecretBundle,
	id_token: &str,
	provider_account_id: &str,
) -> Result<Zeroizing<Vec<u8>>, CodexAuthProjectionError> {
	#[derive(Serialize)]
	struct Tokens<'a> {
		id_token: &'a str,
		access_token: &'a str,
		refresh_token: &'a str,
		account_id: &'a str,
	}

	#[derive(Serialize)]
	struct Auth<'a> {
		auth_mode: &'static str,
		#[serde(rename = "OPENAI_API_KEY")]
		api_key: Option<&'a str>,
		tokens: Tokens<'a>,
		last_refresh: String,
	}

	let auth = Auth {
		auth_mode: "chatgpt",
		api_key: None,
		tokens: Tokens {
			id_token,
			access_token: bundle.access_token(),
			refresh_token: bundle.refresh_token(),
			account_id: provider_account_id,
		},
		last_refresh: now_rfc3339()?,
	};
	let mut encoded = serde_json::to_vec(&auth)
		.map(Zeroizing::new)
		.map_err(|_| CodexAuthProjectionError::Unavailable)?;
	if encoded.len() > MAX_AUTH_FILE_BYTES {
		return Err(CodexAuthProjectionError::InvalidCredential);
	}
	encoded.push(b'\n');
	Ok(encoded)
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct ExistingAuth {
	auth_mode: String,
	#[serde(rename = "OPENAI_API_KEY")]
	api_key: Option<String>,
	tokens: ExistingTokens,
	#[serde(rename = "last_refresh")]
	_last_refresh: String,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct ExistingTokens {
	id_token: String,
	access_token: String,
	refresh_token: String,
	account_id: String,
}

fn current_auth_matches(
	directory: &File,
	bundle: &CredentialSecretBundle,
	id_token: &str,
	provider_account_id: &str,
) -> Result<bool, CodexAuthProjectionError> {
	let Some(expected_identity) = inspect_target(directory)? else {
		return Ok(false);
	};
	let mut target = open_target_readonly(directory)?;
	validate_target_file(&target)?;
	if file_identity(&target)? != expected_identity {
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	let bytes = read_bounded(&mut target)?;
	let Ok(auth) = serde_json::from_slice::<ExistingAuth>(&bytes) else {
		return Ok(false);
	};
	Ok(auth.auth_mode == "chatgpt"
		&& auth.api_key.is_none()
		&& auth.tokens.account_id == provider_account_id
		&& auth.tokens.id_token == id_token
		&& auth.tokens.access_token == bundle.access_token()
		&& auth.tokens.refresh_token == bundle.refresh_token())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityOnlyAuth {
	auth_mode: String,
	#[serde(rename = "OPENAI_API_KEY", default)]
	_api_key: Option<IgnoredAny>,
	#[serde(default, rename = "tokens")]
	_tokens: Option<IdentityOnlyTokens>,
	#[serde(rename = "last_refresh", default)]
	_last_refresh: Option<IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityOnlyTokens {
	#[serde(rename = "id_token", default)]
	_id_token: Option<IgnoredAny>,
	#[serde(rename = "access_token", default)]
	_access_token: Option<IgnoredAny>,
	#[serde(rename = "refresh_token", default)]
	_refresh_token: Option<IgnoredAny>,
	#[serde(rename = "account_id")]
	_account_id: String,
}

#[cfg(test)]
fn read_identity_from_directory(
	directory: &File,
) -> Result<SharedCodexAuthIdentity, CodexAuthProjectionError> {
	let Some(expected_identity) = inspect_target(directory)? else {
		return Ok(SharedCodexAuthIdentity::Unmanaged);
	};
	let mut target = open_target_readonly(directory)?;
	validate_target_file(&target)?;
	if file_identity(&target)? != expected_identity {
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	let bytes = read_bounded(&mut target)?;
	let auth = serde_json::from_slice::<IdentityOnlyAuth>(&bytes)
		.map_err(|_| CodexAuthProjectionError::Unavailable)?;
	if auth.auth_mode != "chatgpt" {
		return Ok(SharedCodexAuthIdentity::Unmanaged);
	}
	let account_id = auth
		._tokens
		.map(|tokens| tokens._account_id)
		.filter(|account_id| {
			!account_id.is_empty()
				&& account_id.len() <= 512
				&& !account_id.chars().any(char::is_control)
		})
		.ok_or(CodexAuthProjectionError::Unavailable)?;
	Ok(SharedCodexAuthIdentity::Chatgpt { provider_account_id: account_id })
}

fn read_snapshot_from_directory(
	directory: &File,
	expected: &SharedCodexAuthFileStamp,
) -> Result<SharedCodexAuthSnapshot, CodexAuthProjectionError> {
	let actual = read_file_stamp(directory)?;
	if &actual != expected {
		return Err(CodexAuthProjectionError::SourceChanged);
	}
	if matches!(actual, SharedCodexAuthFileStamp::Absent) {
		return Ok(SharedCodexAuthSnapshot::Unmanaged {
			version: SharedCodexAuthVersion { stamp: actual, sha256: None },
		});
	}

	let mut target = open_target_readonly(directory)?;
	validate_target_file(&target)?;
	if file_stamp(&target)? != actual {
		return Err(CodexAuthProjectionError::SourceChanged);
	}
	let bytes = read_bounded(&mut target)?;
	if file_stamp(&target)? != actual || read_file_stamp(directory)? != actual {
		return Err(CodexAuthProjectionError::SourceChanged);
	}
	let version =
		SharedCodexAuthVersion { stamp: actual, sha256: Some(Sha256::digest(&bytes).into()) };
	let auth = serde_json::from_slice::<IdentityOnlyAuth>(&bytes)
		.map_err(|_| CodexAuthProjectionError::Unavailable)?;
	if auth.auth_mode != "chatgpt" {
		return Ok(SharedCodexAuthSnapshot::Unmanaged { version });
	}
	let credential =
		parse_shared_codex(&bytes).map_err(|_| CodexAuthProjectionError::Unavailable)?;
	Ok(SharedCodexAuthSnapshot::Managed { version, credential })
}

fn read_file_version(directory: &File) -> Result<SharedCodexAuthVersion, CodexAuthProjectionError> {
	let stamp = read_file_stamp(directory)?;
	if matches!(stamp, SharedCodexAuthFileStamp::Absent) {
		return Ok(SharedCodexAuthVersion { stamp, sha256: None });
	}
	let mut target = open_target_readonly(directory)?;
	validate_target_file(&target)?;
	if file_stamp(&target)? != stamp {
		return Err(CodexAuthProjectionError::SourceChanged);
	}
	let bytes = read_bounded(&mut target)?;
	if file_stamp(&target)? != stamp || read_file_stamp(directory)? != stamp {
		return Err(CodexAuthProjectionError::SourceChanged);
	}
	Ok(SharedCodexAuthVersion { stamp, sha256: Some(Sha256::digest(&bytes).into()) })
}

fn read_file_stamp(directory: &File) -> Result<SharedCodexAuthFileStamp, CodexAuthProjectionError> {
	if inspect_target(directory)?.is_none() {
		return Ok(SharedCodexAuthFileStamp::Absent);
	}
	let target = open_target_readonly(directory)?;
	validate_target_file(&target)?;
	let stamp = file_stamp(&target)?;
	if inspect_target(directory)? != Some(file_identity(&target)?) {
		return Err(CodexAuthProjectionError::SourceChanged);
	}
	Ok(stamp)
}

fn file_stamp(file: &File) -> Result<SharedCodexAuthFileStamp, CodexAuthProjectionError> {
	let metadata = file.metadata().map_err(|_| CodexAuthProjectionError::Unavailable)?;
	Ok(SharedCodexAuthFileStamp::Present {
		device: metadata.dev(),
		inode: metadata.ino(),
		length: metadata.size(),
		modified_seconds: metadata.mtime(),
		modified_nanoseconds: metadata.mtime_nsec(),
	})
}

fn read_bounded(file: &mut File) -> Result<Zeroizing<Vec<u8>>, CodexAuthProjectionError> {
	let mut bytes = Zeroizing::new(Vec::new());
	file.take((MAX_AUTH_FILE_BYTES + 1) as u64)
		.read_to_end(&mut bytes)
		.map_err(|_| CodexAuthProjectionError::Unavailable)?;
	if bytes.len() > MAX_AUTH_FILE_BYTES {
		return Err(CodexAuthProjectionError::Unavailable);
	}
	Ok(bytes)
}

fn now_rfc3339() -> Result<String, CodexAuthProjectionError> {
	let seconds = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_err(|_| CodexAuthProjectionError::Unavailable)?
		.as_secs();
	let seconds: libc::time_t =
		seconds.try_into().map_err(|_| CodexAuthProjectionError::Unavailable)?;
	let mut broken_down = MaybeUninit::<libc::tm>::zeroed();
	if unsafe { libc::gmtime_r(&seconds, broken_down.as_mut_ptr()) }.is_null() {
		return Err(CodexAuthProjectionError::Unavailable);
	}
	let broken_down = unsafe { broken_down.assume_init() };
	Ok(format!(
		"{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
		broken_down.tm_year + 1900,
		broken_down.tm_mon + 1,
		broken_down.tm_mday,
		broken_down.tm_hour,
		broken_down.tm_min,
		broken_down.tm_sec,
	))
}

fn open_codex_directory(home: &Path) -> Result<File, CodexAuthProjectionError> {
	#[cfg(test)]
	if std::env::var_os("DECODEX_CANDIDATE_SANDBOX").as_deref() == Some(std::ffi::OsStr::new("1")) {
		let root = std::env::var_os("TMPDIR")
			.filter(|value| !value.is_empty())
			.map(PathBuf::from)
			.ok_or(CodexAuthProjectionError::UnsafePath)?;
		return open_pinned_sandbox_codex_directory(home, &root);
	}

	if !home.is_absolute() {
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	let root = open_directory_path(Path::new("/"))?;
	validate_ancestor(&root, false)?;
	let mut current = root;
	for component in home.components() {
		match component {
			Component::RootDir => {},
			Component::Normal(name) => {
				current = open_directory_at(current.as_raw_fd(), name.as_bytes())?;
				validate_ancestor(&current, false)?;
			},
			_ => return Err(CodexAuthProjectionError::UnsafePath),
		}
	}
	validate_ancestor(&current, true)?;
	let codex = open_directory_at(current.as_raw_fd(), CODEX_DIRECTORY_NAME.to_bytes())?;
	validate_ancestor(&codex, true)?;
	Ok(codex)
}

#[cfg(test)]
fn open_pinned_sandbox_codex_directory(
	home: &Path,
	root: &Path,
) -> Result<File, CodexAuthProjectionError> {
	if !root.is_absolute() || home == root || !home.starts_with(root) {
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	let mut current = open_exact_sandbox_root(root)?;
	let relative = home.strip_prefix(root).map_err(|_| CodexAuthProjectionError::UnsafePath)?;
	for component in relative.components() {
		let Component::Normal(name) = component else {
			return Err(CodexAuthProjectionError::UnsafePath);
		};
		current = open_directory_at(current.as_raw_fd(), name.as_bytes())?;
		validate_ancestor(&current, true)?;
	}
	let codex = open_directory_at(current.as_raw_fd(), CODEX_DIRECTORY_NAME.to_bytes())?;
	validate_ancestor(&codex, true)?;
	Ok(codex)
}

#[cfg(test)]
fn open_exact_sandbox_root(root: &Path) -> Result<File, CodexAuthProjectionError> {
	open_exact_sandbox_root_after_metadata(root, || {})
}

#[cfg(test)]
fn open_exact_sandbox_root_after_metadata<F>(
	root: &Path,
	after_metadata: F,
) -> Result<File, CodexAuthProjectionError>
where
	F: FnOnce(),
{
	if !root.is_absolute() {
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	let before =
		std::fs::symlink_metadata(root).map_err(|_| CodexAuthProjectionError::UnsafePath)?;
	validate_exact_sandbox_root(&before)?;
	after_metadata();
	let current = open_directory_path(root)?;
	let after = current.metadata().map_err(|_| CodexAuthProjectionError::UnsafePath)?;
	validate_exact_sandbox_root(&after)?;
	if before.dev() != after.dev() || before.ino() != after.ino() {
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	Ok(current)
}

#[cfg(test)]
fn validate_exact_sandbox_root(
	metadata: &std::fs::Metadata,
) -> Result<(), CodexAuthProjectionError> {
	let effective_uid = unsafe { libc::geteuid() };
	if !metadata.is_dir() || metadata.uid() != effective_uid || metadata.mode() & 0o7777 != 0o700 {
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	Ok(())
}

#[cfg(test)]
struct PinnedSandboxFixtureHome {
	root: File,
	home: File,
	codex: File,
	path: PathBuf,
	name: CString,
}

#[cfg(test)]
impl PinnedSandboxFixtureHome {
	fn new() -> Result<Self, CodexAuthProjectionError> {
		let root_path = std::env::var_os("TMPDIR")
			.filter(|value| !value.is_empty())
			.map(PathBuf::from)
			.ok_or(CodexAuthProjectionError::UnsafePath)?;
		let root = open_exact_sandbox_root(&root_path)?;
		let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
		let name =
			CString::new(format!("auth-projection-fixture-{}-{sequence}", std::process::id()))
				.map_err(|_| CodexAuthProjectionError::UnsafePath)?;
		let home = create_exact_private_directory_at(&root, &name)?;
		let codex = match create_exact_private_directory_at(&home, CODEX_DIRECTORY_NAME) {
			Ok(codex) => codex,
			Err(error) => {
				let _ =
					unsafe { libc::unlinkat(root.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) };
				return Err(error);
			},
		};
		let path = root_path.join(std::ffi::OsStr::from_bytes(name.to_bytes()));
		Ok(Self { root, home, codex, path, name })
	}

	fn path(&self) -> &Path {
		&self.path
	}
}

#[cfg(test)]
impl Drop for PinnedSandboxFixtureHome {
	fn drop(&mut self) {
		let _ = unsafe { libc::unlinkat(self.codex.as_raw_fd(), AUTH_FILE_NAME.as_ptr(), 0) };
		let _ = unsafe { libc::unlinkat(self.home.as_raw_fd(), c"outside".as_ptr(), 0) };
		if directory_entry_matches(&self.home, CODEX_DIRECTORY_NAME, &self.codex) {
			let _ = unsafe {
				libc::unlinkat(
					self.home.as_raw_fd(),
					CODEX_DIRECTORY_NAME.as_ptr(),
					libc::AT_REMOVEDIR,
				)
			};
		}
		if directory_entry_matches(&self.root, &self.name, &self.home) {
			let _ = unsafe {
				libc::unlinkat(self.root.as_raw_fd(), self.name.as_ptr(), libc::AT_REMOVEDIR)
			};
		}
	}
}

#[cfg(test)]
fn create_exact_private_directory_at(
	parent: &File,
	name: &CStr,
) -> Result<File, CodexAuthProjectionError> {
	if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	let directory = match open_directory_at(parent.as_raw_fd(), name.to_bytes()) {
		Ok(directory) => directory,
		Err(error) => {
			let _ =
				unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) };
			return Err(error);
		},
	};
	if unsafe { libc::fchmod(directory.as_raw_fd(), 0o700) } != 0
		|| validate_exact_sandbox_root(
			&directory.metadata().map_err(|_| CodexAuthProjectionError::UnsafePath)?,
		)
		.is_err()
	{
		let _ = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) };
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	Ok(directory)
}

#[cfg(test)]
fn directory_entry_matches(parent: &File, name: &CStr, expected: &File) -> bool {
	let Ok(current) = open_directory_at(parent.as_raw_fd(), name.to_bytes()) else {
		return false;
	};
	let (Ok(current), Ok(expected)) = (current.metadata(), expected.metadata()) else {
		return false;
	};
	current.dev() == expected.dev() && current.ino() == expected.ino()
}

fn open_directory_path(path: &Path) -> Result<File, CodexAuthProjectionError> {
	let path = CString::new(path.as_os_str().as_bytes())
		.map_err(|_| CodexAuthProjectionError::UnsafePath)?;
	let descriptor = unsafe {
		libc::open(
			path.as_ptr(),
			libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
		)
	};
	file_from_descriptor(descriptor)
}

fn open_directory_at(parent: RawFd, name: &[u8]) -> Result<File, CodexAuthProjectionError> {
	let name = CString::new(name).map_err(|_| CodexAuthProjectionError::UnsafePath)?;
	let descriptor = unsafe {
		libc::openat(
			parent,
			name.as_ptr(),
			libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
		)
	};
	file_from_descriptor(descriptor)
}

fn file_from_descriptor(descriptor: RawFd) -> Result<File, CodexAuthProjectionError> {
	if descriptor < 0 {
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	Ok(unsafe { File::from_raw_fd(descriptor) })
}

fn validate_ancestor(
	file: &File,
	require_current_user: bool,
) -> Result<(), CodexAuthProjectionError> {
	let metadata = file.metadata().map_err(|_| CodexAuthProjectionError::UnsafePath)?;
	let effective_uid = unsafe { libc::geteuid() };
	if !metadata.is_dir()
		|| metadata.mode() & 0o022 != 0
		|| if require_current_user {
			metadata.uid() != effective_uid
		} else {
			metadata.uid() != 0 && metadata.uid() != effective_uid
		} {
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TargetIdentity {
	device: u64,
	inode: u64,
}

fn directory_identity(file: &File) -> Result<TargetIdentity, CodexAuthProjectionError> {
	let metadata = file.metadata().map_err(|_| CodexAuthProjectionError::UnsafePath)?;
	Ok(TargetIdentity { device: metadata.dev(), inode: metadata.ino() })
}

fn file_identity(file: &File) -> Result<TargetIdentity, CodexAuthProjectionError> {
	let metadata = file.metadata().map_err(|_| CodexAuthProjectionError::UnsafePath)?;
	Ok(TargetIdentity { device: metadata.dev(), inode: metadata.ino() })
}

fn inspect_target(directory: &File) -> Result<Option<TargetIdentity>, CodexAuthProjectionError> {
	let mut status = MaybeUninit::<libc::stat>::zeroed();
	if unsafe {
		libc::fstatat(
			directory.as_raw_fd(),
			AUTH_FILE_NAME.as_ptr(),
			status.as_mut_ptr(),
			libc::AT_SYMLINK_NOFOLLOW,
		)
	} != 0
	{
		return if io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
			Ok(None)
		} else {
			Err(CodexAuthProjectionError::UnsafePath)
		};
	}
	let status = unsafe { status.assume_init() };
	let effective_uid = unsafe { libc::geteuid() };
	if status.st_mode & libc::S_IFMT != libc::S_IFREG
		|| status.st_uid != effective_uid
		|| status.st_mode & 0o7777 != 0o600
		|| status.st_nlink != 1
	{
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	Ok(Some(TargetIdentity { device: status.st_dev as u64, inode: status.st_ino }))
}

fn create_temporary(directory: &File) -> Result<(File, CString), CodexAuthProjectionError> {
	for _ in 0..TEMP_CREATE_ATTEMPTS {
		let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
		let name =
			CString::new(format!(".auth.json.decodex-{}-{sequence}.tmp", std::process::id()))
				.expect("fixed temporary name contains no NUL");
		let descriptor = unsafe {
			libc::openat(
				directory.as_raw_fd(),
				name.as_ptr(),
				libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
				0o600,
			)
		};
		if descriptor >= 0 {
			return Ok((unsafe { File::from_raw_fd(descriptor) }, name));
		}
		if io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
			return Err(CodexAuthProjectionError::Unavailable);
		}
	}
	Err(CodexAuthProjectionError::Unavailable)
}

fn open_target(directory: &File) -> Result<File, CodexAuthProjectionError> {
	let descriptor = unsafe {
		libc::openat(
			directory.as_raw_fd(),
			AUTH_FILE_NAME.as_ptr(),
			libc::O_RDWR | libc::O_CLOEXEC | libc::O_NOFOLLOW,
		)
	};
	if descriptor < 0 {
		return Err(CodexAuthProjectionError::Unavailable);
	}
	Ok(unsafe { File::from_raw_fd(descriptor) })
}

fn open_target_readonly(directory: &File) -> Result<File, CodexAuthProjectionError> {
	let descriptor = unsafe {
		libc::openat(
			directory.as_raw_fd(),
			AUTH_FILE_NAME.as_ptr(),
			libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
		)
	};
	if descriptor < 0 {
		return Err(CodexAuthProjectionError::Unavailable);
	}
	Ok(unsafe { File::from_raw_fd(descriptor) })
}

fn set_exact_mode(file: &File) -> Result<(), CodexAuthProjectionError> {
	if unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
		return Err(CodexAuthProjectionError::Unavailable);
	}
	Ok(())
}

fn validate_target_file(file: &File) -> Result<(), CodexAuthProjectionError> {
	let metadata = file.metadata().map_err(|_| CodexAuthProjectionError::Unavailable)?;
	if !metadata.is_file()
		|| metadata.uid() != unsafe { libc::geteuid() }
		|| metadata.mode() & 0o7777 != 0o600
		|| metadata.nlink() != 1
	{
		return Err(CodexAuthProjectionError::UnsafePath);
	}
	Ok(())
}

fn readback_exact(file: &mut File, expected: &[u8]) -> io::Result<()> {
	let mut actual = Zeroizing::new(Vec::new());
	file.take((MAX_AUTH_FILE_BYTES + 1) as u64).read_to_end(&mut actual)?;
	if actual.as_slice() != expected {
		return Err(io::Error::other("Codex auth readback differs"));
	}
	Ok(())
}

struct TemporaryEntry {
	directory: RawFd,
	name: CString,
	renamed: bool,
}
impl Drop for TemporaryEntry {
	fn drop(&mut self) {
		if !self.renamed {
			let _ = unsafe { libc::unlinkat(self.directory, self.name.as_ptr(), 0) };
		}
	}
}

/// Closed projection failure. No variant contains a path or credential material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CodexAuthProjectionError {
	UnsafePath,
	Unavailable,
	OutcomeUnknown,
	SourceChanged,
	MissingIdentityToken,
	InvalidCredential,
}

/// Credential-negative identity read from the normal shared Codex auth file.
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SharedCodexAuthIdentity {
	Chatgpt {
		/// Non-secret provider account identity.
		provider_account_id: String,
	},
	Unmanaged,
}

#[cfg(test)]
mod tests {
	use std::{
		fs,
		os::unix::fs::{MetadataExt as _, PermissionsExt as _, symlink},
		path::Path,
	};

	use serde_json::Value;
	use tempfile::tempdir_in;

	use super::{
		CodexAuthProjectionError, PinnedSandboxFixtureHome, ProjectionFault,
		SharedCodexAuthIdentity, current_auth_matches, open_codex_directory,
		open_exact_sandbox_root, open_exact_sandbox_root_after_metadata,
		open_pinned_sandbox_codex_directory, project_shared_codex_auth_at,
		project_shared_codex_auth_cas_at, project_shared_codex_auth_with_precondition_at,
		project_to_directory, read_file_stamp, read_identity_from_directory,
		read_snapshot_from_directory,
	};
	use crate::host_credentials::CredentialSecretBundle;

	enum FixtureHome {
		Ordinary(tempfile::TempDir),
		Sandboxed(PinnedSandboxFixtureHome),
	}

	impl FixtureHome {
		fn path(&self) -> &Path {
			match self {
				Self::Ordinary(home) => home.path(),
				Self::Sandboxed(home) => home.path(),
			}
		}
	}

	fn bundle(id_token: Option<&str>, suffix: &str) -> CredentialSecretBundle {
		CredentialSecretBundle::chatgpt(
			format!("access-{suffix}"),
			format!("refresh-{suffix}"),
			id_token.map(str::to_owned),
			Some("pro".to_owned()),
			format!("{suffix}@example.test"),
			"bearer".to_owned(),
			4_000_000,
		)
		.unwrap()
	}

	fn fixture_home() -> FixtureHome {
		if std::env::var_os("DECODEX_CANDIDATE_SANDBOX").as_deref()
			== Some(std::ffi::OsStr::new("1"))
		{
			FixtureHome::Sandboxed(PinnedSandboxFixtureHome::new().unwrap())
		} else {
			let home = tempdir_in(std::env::current_dir().unwrap()).unwrap();
			fs::create_dir(home.path().join(".codex")).unwrap();
			FixtureHome::Ordinary(home)
		}
	}

	#[test]
	fn sandbox_codex_root_is_exact_private_and_descriptor_pinned() {
		let fixture = fixture_home();
		let root = fixture.path().join("sandbox");
		let home = root.join("home");
		fs::create_dir(&root).unwrap();
		fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
		fs::create_dir(&home).unwrap();
		fs::create_dir(home.join(".codex")).unwrap();

		open_pinned_sandbox_codex_directory(&home, &root).unwrap();
		assert!(open_pinned_sandbox_codex_directory(fixture.path(), &root).is_err());
		fs::set_permissions(&root, fs::Permissions::from_mode(0o1700)).unwrap();
		assert!(open_pinned_sandbox_codex_directory(&home, &root).is_err());
		fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();

		let root_link = fixture.path().join("root-link");
		symlink(&root, &root_link).unwrap();
		assert!(open_exact_sandbox_root(&root_link).is_err());
		fs::remove_file(&root_link).unwrap();

		let retained = fixture.path().join("retained-root");
		assert!(
			open_exact_sandbox_root_after_metadata(&root, || {
				fs::rename(&root, &retained).unwrap();
				symlink(&retained, &root).unwrap();
			})
			.is_err()
		);
		fs::remove_file(&root).unwrap();
		fs::rename(&retained, &root).unwrap();

		let symlink_home = root.join("symlink-home");
		symlink(&home, &symlink_home).unwrap();
		assert!(open_pinned_sandbox_codex_directory(&symlink_home, &root).is_err());
		fs::remove_file(symlink_home).unwrap();

		let linked_codex_home = root.join("linked-codex-home");
		fs::create_dir(&linked_codex_home).unwrap();
		symlink(home.join(".codex"), linked_codex_home.join(".codex")).unwrap();
		assert!(open_pinned_sandbox_codex_directory(&linked_codex_home, &root).is_err());
		fs::remove_file(linked_codex_home.join(".codex")).unwrap();
		fs::remove_dir(linked_codex_home).unwrap();
		fs::remove_dir(home.join(".codex")).unwrap();
		fs::remove_dir(home).unwrap();
		fs::remove_dir(root).unwrap();
	}

	fn read_auth(home: &Path) -> Value {
		let bytes = fs::read(home.join(".codex/auth.json")).unwrap();
		serde_json::from_slice(&bytes).unwrap()
	}

	#[test]
	fn projection_writes_only_the_native_codex_auth_shape_and_is_idempotent() {
		let home = fixture_home();
		let directory = open_codex_directory(home.path()).unwrap();

		project_to_directory(&directory, &bundle(Some("id-one"), "one"), "provider-one").unwrap();
		let first_inode = fs::metadata(home.path().join(".codex/auth.json")).unwrap().ino();
		project_to_directory(&directory, &bundle(Some("id-one"), "one"), "provider-one").unwrap();
		let second_inode = fs::metadata(home.path().join(".codex/auth.json")).unwrap().ino();

		let auth = read_auth(home.path());
		assert_eq!(first_inode, second_inode);
		let mut keys = auth.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>();
		keys.sort_unstable();
		assert_eq!(keys, ["OPENAI_API_KEY", "auth_mode", "last_refresh", "tokens"]);
		assert_eq!(auth["auth_mode"], "chatgpt");
		assert!(auth["OPENAI_API_KEY"].is_null());
		let mut token_keys =
			auth["tokens"].as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>();
		token_keys.sort_unstable();
		assert_eq!(token_keys, ["access_token", "account_id", "id_token", "refresh_token"]);
		assert_eq!(auth["tokens"]["account_id"], "provider-one");
		let mode = fs::metadata(home.path().join(".codex/auth.json")).unwrap().permissions().mode();
		assert_eq!(mode & 0o777, 0o600);
	}

	#[test]
	fn identity_readback_distinguishes_absent_and_managed_without_returning_tokens() {
		let home = fixture_home();
		let directory = open_codex_directory(home.path()).unwrap();
		assert_eq!(
			read_identity_from_directory(&directory).unwrap(),
			SharedCodexAuthIdentity::Unmanaged,
		);

		project_to_directory(&directory, &bundle(Some("id-one"), "one"), "provider-one").unwrap();
		assert_eq!(
			read_identity_from_directory(&directory).unwrap(),
			SharedCodexAuthIdentity::Chatgpt { provider_account_id: "provider-one".to_owned() },
		);
	}

	#[test]
	fn exact_readback_rejects_stale_tokens_for_the_same_provider_identity() {
		let home = fixture_home();
		let directory = open_codex_directory(home.path()).unwrap();
		project_to_directory(&directory, &bundle(Some("id-one"), "one"), "provider-one").unwrap();

		assert!(
			current_auth_matches(
				&directory,
				&bundle(Some("id-one"), "one"),
				"id-one",
				"provider-one",
			)
			.unwrap()
		);
		assert!(
			!current_auth_matches(
				&directory,
				&bundle(Some("id-two"), "two"),
				"id-two",
				"provider-one",
			)
			.unwrap()
		);
	}

	#[test]
	fn later_projection_replaces_a_stale_bundle_for_the_same_provider_identity() {
		let home = fixture_home();
		let directory = open_codex_directory(home.path()).unwrap();
		let initial = bundle(Some("id-one"), "one");
		let rotated = bundle(Some("id-two"), "two");
		project_to_directory(&directory, &initial, "provider-one").unwrap();

		project_to_directory(&directory, &rotated, "provider-one").unwrap();

		assert!(current_auth_matches(&directory, &rotated, "id-two", "provider-one").unwrap());
		assert!(!current_auth_matches(&directory, &initial, "id-one", "provider-one").unwrap());
	}

	#[test]
	fn projection_atomically_replaces_another_account() {
		let home = fixture_home();
		let directory = open_codex_directory(home.path()).unwrap();

		project_to_directory(&directory, &bundle(Some("id-one"), "one"), "provider-one").unwrap();
		project_to_directory(&directory, &bundle(Some("id-two"), "two"), "provider-two").unwrap();

		let auth = read_auth(home.path());
		assert_eq!(auth["tokens"]["account_id"], "provider-two");
		assert_eq!(auth["tokens"]["id_token"], "id-two");
	}

	#[test]
	fn conditional_projection_preserves_a_source_that_changed_before_replace() {
		let home = fixture_home();
		let directory = open_codex_directory(home.path()).unwrap();
		let first = bundle(Some("id-one"), "one");
		let second = bundle(Some("id-two"), "two");
		let concurrent = bundle(Some("id-three"), "three");
		project_to_directory(&directory, &first, "provider-one").unwrap();

		project_shared_codex_auth_with_precondition_at(
			home.path(),
			&second,
			"provider-two",
			Some((&first, "provider-one")),
			ProjectionFault::None,
		)
		.unwrap();
		assert_eq!(read_auth(home.path())["tokens"]["account_id"], "provider-two");

		project_to_directory(&directory, &concurrent, "provider-three").unwrap();
		assert_eq!(
			project_shared_codex_auth_with_precondition_at(
				home.path(),
				&second,
				"provider-two",
				Some((&first, "provider-one")),
				ProjectionFault::None,
			),
			Err(CodexAuthProjectionError::SourceChanged),
		);
		assert_eq!(read_auth(home.path())["tokens"]["account_id"], "provider-three");
	}

	#[test]
	fn exact_version_cas_preserves_changed_unmanaged_and_absent_sources() {
		let home = fixture_home();
		let directory = open_codex_directory(home.path()).unwrap();
		let target = home.path().join(".codex/auth.json");
		let absent_stamp = read_file_stamp(&directory).unwrap();
		let absent = read_snapshot_from_directory(&directory, &absent_stamp).unwrap();
		fs::write(&target, br#"{"auth_mode":"apikey","OPENAI_API_KEY":null}"#).unwrap();
		fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
		assert_eq!(
			project_shared_codex_auth_cas_at(
				home.path(),
				&bundle(Some("id-target"), "target"),
				"provider-target",
				absent.version(),
				ProjectionFault::None,
			),
			Err(CodexAuthProjectionError::SourceChanged),
		);

		let unmanaged_stamp = read_file_stamp(&directory).unwrap();
		let unmanaged = read_snapshot_from_directory(&directory, &unmanaged_stamp).unwrap();
		fs::write(&target, br#"{"auth_mode":"apikey","OPENAI_API_KEY":"changed"}"#).unwrap();
		assert_eq!(
			project_shared_codex_auth_cas_at(
				home.path(),
				&bundle(Some("id-target"), "target"),
				"provider-target",
				unmanaged.version(),
				ProjectionFault::None,
			),
			Err(CodexAuthProjectionError::SourceChanged),
		);
		assert_eq!(read_auth(home.path())["OPENAI_API_KEY"], "changed");
	}

	#[test]
	fn partial_shared_auth_is_unavailable_and_is_never_interpreted_as_absent() {
		let home = fixture_home();
		let directory = open_codex_directory(home.path()).unwrap();
		let target = home.path().join(".codex/auth.json");
		fs::write(&target, b"{").unwrap();
		fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
		let stamp = read_file_stamp(&directory).unwrap();
		assert!(matches!(
			read_snapshot_from_directory(&directory, &stamp),
			Err(CodexAuthProjectionError::Unavailable)
		));
		assert_eq!(fs::read(&target).unwrap(), b"{");
	}

	#[test]
	fn pre_rename_failure_is_definite_and_leaves_no_projection() {
		let home = fixture_home();

		assert_eq!(
			project_shared_codex_auth_at(
				home.path(),
				&bundle(Some("id-before"), "before"),
				"provider-before",
				ProjectionFault::BeforeRename,
			),
			Err(CodexAuthProjectionError::Unavailable),
		);
		assert!(!home.path().join(".codex/auth.json").exists());
	}

	#[test]
	fn every_post_rename_failure_is_unknown_and_same_request_reconciles_exactly() {
		for fault in [
			ProjectionFault::AfterRenameOpen,
			ProjectionFault::AfterRenameFileSync,
			ProjectionFault::AfterRenameReadback,
			ProjectionFault::AfterRenameParentSync,
			ProjectionFault::AfterRenamePathRevalidation,
		] {
			let home = fixture_home();
			let bundle = bundle(Some("id-reconcile"), "reconcile");

			assert_eq!(
				project_shared_codex_auth_at(home.path(), &bundle, "provider-reconcile", fault,),
				Err(CodexAuthProjectionError::OutcomeUnknown),
				"{fault:?}",
			);

			let directory = open_codex_directory(home.path()).unwrap();
			assert!(
				current_auth_matches(&directory, &bundle, "id-reconcile", "provider-reconcile",)
					.unwrap(),
				"{fault:?}",
			);
			project_shared_codex_auth_at(
				home.path(),
				&bundle,
				"provider-reconcile",
				ProjectionFault::None,
			)
			.unwrap();
		}
	}

	#[test]
	fn projection_rejects_missing_id_token() {
		let home = fixture_home();
		let directory = open_codex_directory(home.path()).unwrap();

		assert_eq!(
			project_to_directory(&directory, &bundle(None, "missing"), "provider-missing"),
			Err(CodexAuthProjectionError::MissingIdentityToken),
		);
		assert!(!home.path().join(".codex/auth.json").exists());
	}

	#[test]
	fn projection_rejects_symlink_and_unsafe_existing_mode() {
		let symlink_home = fixture_home();
		let outside = symlink_home.path().join("outside");
		fs::write(&outside, b"outside").unwrap();
		symlink(&outside, symlink_home.path().join(".codex/auth.json")).unwrap();
		let directory = open_codex_directory(symlink_home.path()).unwrap();
		assert_eq!(
			project_to_directory(&directory, &bundle(Some("id"), "symlink"), "provider"),
			Err(CodexAuthProjectionError::UnsafePath),
		);
		assert_eq!(fs::read(&outside).unwrap(), b"outside");

		let mode_home = fixture_home();
		let auth = mode_home.path().join(".codex/auth.json");
		fs::write(&auth, b"{}").unwrap();
		fs::set_permissions(&auth, fs::Permissions::from_mode(0o644)).unwrap();
		let directory = open_codex_directory(mode_home.path()).unwrap();
		assert_eq!(
			project_to_directory(&directory, &bundle(Some("id"), "mode"), "provider"),
			Err(CodexAuthProjectionError::UnsafePath),
		);
		assert_eq!(fs::metadata(auth).unwrap().permissions().mode() & 0o777, 0o644);
	}

	#[test]
	fn projection_rejects_an_unsafe_codex_parent() {
		let home = fixture_home();
		let codex = home.path().join(".codex");
		fs::set_permissions(&codex, fs::Permissions::from_mode(0o777)).unwrap();

		assert!(matches!(
			open_codex_directory(home.path()),
			Err(CodexAuthProjectionError::UnsafePath)
		));
	}
}
