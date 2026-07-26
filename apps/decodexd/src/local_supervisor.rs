//! Unix-local lifecycle owner for PostgreSQL and the credential-injected daemon.

use std::{
	collections::{BTreeMap, BTreeSet},
	error::Error,
	ffi::OsString,
	fmt::{Display, Formatter},
	fs::{self, File, OpenOptions},
	io::{Read as _, Write as _},
	os::unix::fs::{FileTypeExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
	path::{Path, PathBuf},
	process::{Child, Command, ExitStatus, Stdio},
	thread,
	time::{Duration, Instant},
};

use base64::{
	Engine as _,
	engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use zeroize::{Zeroize as _, Zeroizing};

use crate::ShutdownSignals;

const MAPPING_SCHEMA: &str = "decodex/reset-card-legacy-bridge/1";
const MAX_ACCOUNTS: usize = 64;
const MAX_MAPPING_BYTES: u64 = 64 * 1024;
const MAX_ACCOUNTS_BYTES: u64 = 4 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 128 * 1024;
const MAX_ACCESS_TOKEN_BYTES: usize = 64 * 1024;
const MAX_ID_TOKEN_BYTES: usize = 64 * 1024;
const MAX_PROVIDER_ACCOUNT_ID_BYTES: usize = 1_024;
const MAX_EMAIL_BYTES: usize = 320;
const MAX_PLAN_TYPE_BYTES: usize = 64;
const CREDENTIAL_PROJECTION_FINGERPRINT_PROTOCOL: &[u8] =
	b"decodex/local-supervisor-credential-projection/1\0";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
// Reset-card provider work has a runtime-owned 212-second upper bound. The daemon grace period
// also covers its five-second transport drain and leaves margin for scheduler and cleanup work.
// The macOS installer signals a loaded non-restarting generation and lets these runtime bounds
// settle it before bootout. Launchd retains a 60-second final stop fallback for other stop paths.
const DAEMON_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(240);
const POSTGRES_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub(super) struct LocalSupervisorConfig {
	pub(super) postgres: PathBuf,
	pub(super) pg_isready: PathBuf,
	pub(super) data_directory: PathBuf,
	pub(super) socket_directory: PathBuf,
	pub(super) port: u16,
	pub(super) legacy_accounts: PathBuf,
	pub(super) legacy_mapping: PathBuf,
	pub(super) working_directory: PathBuf,
}

pub(super) async fn supervise(config: LocalSupervisorConfig) -> Result<(), SupervisorError> {
	validate_absolute_paths(&config)?;
	if config.port == 0 {
		return Err(SupervisorError::new("PostgreSQL port is invalid"));
	}
	validate_command_path(&config.postgres)?;
	validate_command_path(&config.pg_isready)?;
	validate_private_data_directory(&config.data_directory)?;
	validate_directory(&config.working_directory)?;

	let socket_directory_identity = secure_directory_identity(&config.socket_directory)?;
	let executable =
		std::env::current_exe().map_err(|_| SupervisorError::new("cannot resolve decodexd"))?;
	let mut signals = ShutdownSignals::new()
		.map_err(|_| SupervisorError::new("cannot install supervisor signal handlers"))?;
	let lock_path = account_lock_path(&config.legacy_accounts)?;
	let (mut credentials, mut watched_accounts) =
		load_credentials(&config.legacy_accounts, &lock_path, &config.legacy_mapping)?;
	let mut credential_projection = credential_projection_fingerprint(&credentials);
	let mut postgres = spawn_postgres(&config)?;

	let ready =
		wait_for_postgres(&config, &mut postgres, &mut signals, socket_directory_identity).await;
	let postgres_identity = match ready {
		Ok(PostgresStartup::Ready(identity)) => identity,
		Ok(PostgresStartup::ShutdownRequested) => {
			stop_child(&mut postgres, ChildKind::Postgres);
			return Ok(());
		},
		Err(error) => {
			stop_child(&mut postgres, ChildKind::Postgres);
			return Err(error);
		},
	};
	let mut daemon = match spawn_daemon(&config, &executable, &credentials) {
		Ok(child) => child,
		Err(error) => {
			stop_child(&mut postgres, ChildKind::Postgres);
			return Err(error);
		},
	};

	credentials.clear();
	let _ = writeln!(std::io::stdout().lock(), "decodexd local supervisor ready");

	macro_rules! observe_or_stop {
		($observation:expr) => {
			match $observation {
				Ok(value) => value,
				Err(error) => {
					stop_child(&mut daemon, ChildKind::Daemon);
					stop_child(&mut postgres, ChildKind::Postgres);
					return Err(error);
				},
			}
		};
	}

	loop {
		tokio::select! {
			signal = signals.recv() => {
				if signal.is_err() {
					stop_child(&mut daemon, ChildKind::Daemon);
					stop_child(&mut postgres, ChildKind::Postgres);
					return Err(SupervisorError::new("supervisor signal handling failed"));
				}
				stop_child(&mut daemon, ChildKind::Daemon);
				stop_child(&mut postgres, ChildKind::Postgres);
				return Ok(());
			},
			_ = tokio::time::sleep(POLL_INTERVAL) => {},
		}

		if observe_or_stop!(child_exit(&mut postgres)).is_some() {
			stop_child(&mut daemon, ChildKind::Daemon);
			return Err(SupervisorError::new("PostgreSQL exited"));
		}
		if !observe_or_stop!(postgres_identity.is_current(&config)) {
			stop_child(&mut daemon, ChildKind::Daemon);
			stop_child(&mut postgres, ChildKind::Postgres);
			return Err(SupervisorError::new("PostgreSQL identity changed"));
		}
		if observe_or_stop!(child_exit(&mut daemon)).is_some() {
			stop_child(&mut postgres, ChildKind::Postgres);
			return Err(SupervisorError::new("decodexd child exited"));
		}

		let current = observe_or_stop!(WatchedCredentialFiles::capture(
			&config.legacy_accounts,
			&lock_path,
			&config.legacy_mapping,
		));
		if current != watched_accounts {
			let (mut reloaded, reloaded_watch) = observe_or_stop!(load_credentials(
				&config.legacy_accounts,
				&lock_path,
				&config.legacy_mapping,
			));
			let reloaded_projection = credential_projection_fingerprint(&reloaded);

			if reloaded_projection.as_ref() == credential_projection.as_ref() {
				reloaded.clear();
				watched_accounts = reloaded_watch;

				continue;
			}

			stop_child(&mut daemon, ChildKind::Daemon);
			daemon = match spawn_daemon(&config, &executable, &reloaded) {
				Ok(child) => child,
				Err(error) => {
					stop_child(&mut postgres, ChildKind::Postgres);
					return Err(error);
				},
			};
			reloaded.clear();
			credential_projection = reloaded_projection;
			watched_accounts = reloaded_watch;
		}
	}
}

fn validate_absolute_paths(config: &LocalSupervisorConfig) -> Result<(), SupervisorError> {
	for path in [
		&config.postgres,
		&config.pg_isready,
		&config.data_directory,
		&config.socket_directory,
		&config.legacy_accounts,
		&config.legacy_mapping,
		&config.working_directory,
	] {
		if !path.is_absolute() {
			return Err(SupervisorError::new("local supervisor paths must be absolute"));
		}
	}

	Ok(())
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct FileIdentity {
	device: u64,
	inode: u64,
	uid: u32,
	mode: u32,
	links: u64,
	length: u64,
	modified_seconds: i64,
	modified_nanoseconds: i64,
	changed_seconds: i64,
	changed_nanoseconds: i64,
	kind: FileKind,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum FileKind {
	Directory,
	Regular,
	Socket,
}

impl FileIdentity {
	fn from_metadata(metadata: &fs::Metadata) -> Result<Self, SupervisorError> {
		let file_type = metadata.file_type();
		let kind = if file_type.is_dir() {
			FileKind::Directory
		} else if file_type.is_file() {
			FileKind::Regular
		} else if file_type.is_socket() {
			FileKind::Socket
		} else {
			return Err(SupervisorError::new("unsupported filesystem object"));
		};

		Ok(Self {
			device: metadata.dev(),
			inode: metadata.ino(),
			uid: metadata.uid(),
			mode: metadata.permissions().mode() & 0o777,
			links: metadata.nlink(),
			length: metadata.len(),
			modified_seconds: metadata.mtime(),
			modified_nanoseconds: metadata.mtime_nsec(),
			changed_seconds: metadata.ctime(),
			changed_nanoseconds: metadata.ctime_nsec(),
			kind,
		})
	}

	fn stable_object(self) -> StableObjectIdentity {
		StableObjectIdentity {
			device: self.device,
			inode: self.inode,
			uid: self.uid,
			mode: self.mode,
			kind: self.kind,
		}
	}
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct StableObjectIdentity {
	device: u64,
	inode: u64,
	uid: u32,
	mode: u32,
	kind: FileKind,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct WatchedCredentialFiles {
	parent: StableObjectIdentity,
	accounts: FileIdentity,
	lock: StableObjectIdentity,
	mapping_parent: StableObjectIdentity,
	mapping: FileIdentity,
}

impl WatchedCredentialFiles {
	fn capture(accounts: &Path, lock: &Path, mapping: &Path) -> Result<Self, SupervisorError> {
		Ok(Self {
			parent: secure_parent_identity(accounts)?.stable_object(),
			accounts: secure_regular_file_identity(accounts, MAX_ACCOUNTS_BYTES)?,
			lock: secure_regular_file_identity(lock, 0)?.stable_object(),
			mapping_parent: secure_parent_identity(mapping)?.stable_object(),
			mapping: secure_regular_file_identity(mapping, MAX_MAPPING_BYTES)?,
		})
	}
}

#[derive(Clone, Copy)]
struct PostgresIdentity {
	process_id: u32,
	socket_directory: StableObjectIdentity,
	socket: StableObjectIdentity,
	generation: StableObjectIdentity,
}

enum PostgresStartup {
	Ready(PostgresIdentity),
	ShutdownRequested,
}

impl PostgresIdentity {
	fn is_current(self, config: &LocalSupervisorConfig) -> Result<bool, SupervisorError> {
		let current_directory = secure_directory_identity(&config.socket_directory)?;
		if current_directory != self.socket_directory {
			return Ok(false);
		}

		let current_socket = socket_identity(&postgres_socket_path(config))?;
		if current_socket != self.socket {
			return Ok(false);
		}

		let generation_path = config.data_directory.join("postmaster.pid");
		let current_generation =
			secure_regular_file_identity(&generation_path, 64 * 1024)?.stable_object();
		if current_generation != self.generation {
			return Ok(false);
		}

		Ok(read_postmaster_pid(&generation_path)? == self.process_id)
	}
}

fn validate_command_path(path: &Path) -> Result<(), SupervisorError> {
	let metadata = fs::symlink_metadata(path)
		.map_err(|_| SupervisorError::new("required executable is unavailable"))?;
	if metadata.file_type().is_symlink()
		|| !metadata.is_file()
		|| metadata.permissions().mode() & 0o111 == 0
	{
		return Err(SupervisorError::new("required executable is unsafe"));
	}

	Ok(())
}

fn validate_private_data_directory(path: &Path) -> Result<(), SupervisorError> {
	let metadata = fs::symlink_metadata(path)
		.map_err(|_| SupervisorError::new("PostgreSQL data directory is unavailable"))?;
	// SAFETY: `geteuid` has no arguments or failure return.
	let effective_uid = unsafe { libc::geteuid() };

	if metadata.file_type().is_symlink()
		|| !metadata.is_dir()
		|| metadata.uid() != effective_uid
		|| metadata.permissions().mode() & 0o777 != 0o700
	{
		return Err(SupervisorError::new("PostgreSQL data directory is unsafe"));
	}

	Ok(())
}

fn validate_directory(path: &Path) -> Result<(), SupervisorError> {
	let metadata = fs::symlink_metadata(path)
		.map_err(|_| SupervisorError::new("required directory missing"))?;
	if metadata.file_type().is_symlink() || !metadata.is_dir() {
		return Err(SupervisorError::new("required directory is unsafe"));
	}

	Ok(())
}

fn account_lock_path(accounts: &Path) -> Result<PathBuf, SupervisorError> {
	let parent =
		accounts.parent().ok_or_else(|| SupervisorError::new("legacy account path is invalid"))?;
	let name = accounts
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| SupervisorError::new("legacy account path is invalid"))?;

	Ok(parent.join(format!(".{name}.lock")))
}

fn secure_parent_identity(path: &Path) -> Result<FileIdentity, SupervisorError> {
	let parent =
		path.parent().ok_or_else(|| SupervisorError::new("credential parent is invalid"))?;
	let metadata = fs::symlink_metadata(parent)
		.map_err(|_| SupervisorError::new("credential parent is unavailable"))?;
	let identity = FileIdentity::from_metadata(&metadata)?;
	// SAFETY: `geteuid` has no arguments or failure return.
	let effective_uid = unsafe { libc::geteuid() };

	if metadata.file_type().is_symlink()
		|| identity.kind != FileKind::Directory
		|| identity.uid != effective_uid
		|| identity.mode != 0o700
	{
		return Err(SupervisorError::new("credential parent is unsafe"));
	}

	Ok(identity)
}

fn secure_directory_identity(path: &Path) -> Result<StableObjectIdentity, SupervisorError> {
	let metadata = fs::symlink_metadata(path)
		.map_err(|_| SupervisorError::new("PostgreSQL socket directory is unavailable"))?;
	let identity = FileIdentity::from_metadata(&metadata)?;
	// SAFETY: `geteuid` has no arguments or failure return.
	let effective_uid = unsafe { libc::geteuid() };

	if metadata.file_type().is_symlink()
		|| identity.kind != FileKind::Directory
		|| identity.uid != effective_uid
		|| identity.mode != 0o700
	{
		return Err(SupervisorError::new("PostgreSQL socket directory is unsafe"));
	}

	Ok(identity.stable_object())
}

fn secure_regular_file_identity(
	path: &Path,
	maximum_bytes: u64,
) -> Result<FileIdentity, SupervisorError> {
	let metadata =
		fs::symlink_metadata(path).map_err(|_| SupervisorError::new("credential file missing"))?;
	let identity = FileIdentity::from_metadata(&metadata)?;
	// SAFETY: `geteuid` has no arguments or failure return.
	let effective_uid = unsafe { libc::geteuid() };

	if metadata.file_type().is_symlink()
		|| identity.kind != FileKind::Regular
		|| identity.uid != effective_uid
		|| identity.mode != 0o600
		|| identity.links != 1
		|| identity.length > maximum_bytes
	{
		return Err(SupervisorError::new("credential file is unsafe"));
	}

	Ok(identity)
}

fn open_secure_regular(
	path: &Path,
	maximum_bytes: u64,
) -> Result<(File, FileIdentity), SupervisorError> {
	let expected = secure_regular_file_identity(path, maximum_bytes)?;
	let file = OpenOptions::new()
		.read(true)
		.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
		.open(path)
		.map_err(|_| SupervisorError::new("credential file cannot be opened"))?;
	let actual = FileIdentity::from_metadata(
		&file
			.metadata()
			.map_err(|_| SupervisorError::new("credential file cannot be inspected"))?,
	)?;

	if actual != expected {
		return Err(SupervisorError::new("credential file changed during open"));
	}

	Ok((file, actual))
}

fn load_credentials(
	accounts_path: &Path,
	lock_path: &Path,
	mapping_path: &Path,
) -> Result<(Vec<SlotCredential>, WatchedCredentialFiles), SupervisorError> {
	secure_parent_identity(accounts_path)?;
	secure_parent_identity(mapping_path)?;
	let (lock, lock_identity) = open_secure_regular(lock_path, 0)?;
	acquire_shared_lock(&lock)?;
	let loaded = (|| {
		let before = WatchedCredentialFiles::capture(accounts_path, lock_path, mapping_path)?;
		if before.lock != lock_identity.stable_object() {
			return Err(SupervisorError::new("credential lock changed during acquisition"));
		}
		let mapping = read_mapping(mapping_path)?;
		let accounts = read_legacy_accounts(accounts_path)?;
		let credentials = map_credentials(mapping, accounts)?;
		let after = WatchedCredentialFiles::capture(accounts_path, lock_path, mapping_path)?;
		if before != after {
			return Err(SupervisorError::new("credential files changed during load"));
		}

		Ok((credentials, after))
	})();
	unlock(&lock);

	loaded
}

fn acquire_shared_lock(file: &File) -> Result<(), SupervisorError> {
	use std::os::fd::AsRawFd as _;

	let deadline = Instant::now() + LOCK_TIMEOUT;
	loop {
		// SAFETY: the descriptor remains owned by `file` for the duration of this call.
		let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) };
		if result == 0 {
			return Ok(());
		}
		let error = std::io::Error::last_os_error();
		let error_code = error.raw_os_error();
		if error_code != Some(libc::EWOULDBLOCK) && error_code != Some(libc::EAGAIN) {
			return Err(SupervisorError::new("credential lock failed"));
		}
		if Instant::now() >= deadline {
			return Err(SupervisorError::new("credential lock timed out"));
		}
		thread::sleep(Duration::from_millis(20));
	}
}

fn unlock(file: &File) {
	use std::os::fd::AsRawFd as _;

	// SAFETY: the descriptor remains valid here; dropping it also releases the lock.
	let _ = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MappingManifest {
	schema: String,
	accounts: Vec<MappingEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MappingEntry {
	slot: u8,
	provider_account_id_sha256: String,
}

fn read_mapping(path: &Path) -> Result<Vec<MappingEntry>, SupervisorError> {
	let (file, identity) = open_secure_regular(path, MAX_MAPPING_BYTES)?;
	let mut bytes = Vec::with_capacity(identity.length as usize);
	file.take(MAX_MAPPING_BYTES + 1)
		.read_to_end(&mut bytes)
		.map_err(|_| SupervisorError::new("mapping manifest cannot be read"))?;
	if bytes.len() as u64 > MAX_MAPPING_BYTES {
		return Err(SupervisorError::new("mapping manifest is too large"));
	}
	let manifest = serde_json::from_slice::<MappingManifest>(&bytes)
		.map_err(|_| SupervisorError::new("mapping manifest is malformed"))?;

	if manifest.schema != MAPPING_SCHEMA
		|| manifest.accounts.is_empty()
		|| manifest.accounts.len() > MAX_ACCOUNTS
	{
		return Err(SupervisorError::new("mapping manifest is invalid"));
	}

	let mut slots = BTreeSet::new();
	let mut digests = BTreeSet::new();
	for entry in &manifest.accounts {
		if !(1..=MAX_ACCOUNTS as u8).contains(&entry.slot)
			|| !is_lower_hex_sha256(&entry.provider_account_id_sha256)
			|| !slots.insert(entry.slot)
			|| !digests.insert(entry.provider_account_id_sha256.as_str())
		{
			return Err(SupervisorError::new("mapping manifest is invalid"));
		}
	}
	let expected_slots = (1..=manifest.accounts.len() as u8).collect::<BTreeSet<_>>();
	if slots != expected_slots {
		return Err(SupervisorError::new("mapping manifest slots are not contiguous"));
	}

	Ok(manifest.accounts)
}

fn is_lower_hex_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

struct LegacyLine {
	email: Option<String>,
	tokens: Option<LegacyTokens>,
	auth: Option<LegacyAuth>,
}

#[derive(Deserialize)]
struct LegacyLineSerde {
	email: Option<String>,
	tokens: Option<LegacyTokens>,
	auth: Option<LegacyAuth>,
}

impl From<LegacyLineSerde> for LegacyLine {
	fn from(value: LegacyLineSerde) -> Self {
		Self { email: value.email, tokens: value.tokens, auth: value.auth }
	}
}

impl Drop for LegacyLine {
	fn drop(&mut self) {
		self.email.zeroize();
	}
}

#[derive(Deserialize)]
struct LegacyAuth {
	email: Option<String>,
	tokens: Option<LegacyTokens>,
}

impl Drop for LegacyAuth {
	fn drop(&mut self) {
		self.email.zeroize();
	}
}

#[derive(Deserialize)]
struct LegacyTokens {
	id_token: String,
	access_token: String,
	account_id: Option<String>,
	email: Option<String>,
}

impl Drop for LegacyTokens {
	fn drop(&mut self) {
		self.id_token.zeroize();
		self.access_token.zeroize();
		self.account_id.zeroize();
		self.email.zeroize();
	}
}

#[derive(Deserialize)]
struct IdentityClaims {
	email: Option<String>,
	#[serde(rename = "https://api.openai.com/auth")]
	authority: Option<IdentityAuthority>,
}

impl Drop for IdentityClaims {
	fn drop(&mut self) {
		self.email.zeroize();
	}
}

#[derive(Deserialize)]
struct IdentityAuthority {
	chatgpt_account_id: Option<String>,
	chatgpt_plan_type: Option<String>,
}

impl Drop for IdentityAuthority {
	fn drop(&mut self) {
		self.chatgpt_account_id.zeroize();
		self.chatgpt_plan_type.zeroize();
	}
}

struct LegacyCredential {
	access_token: Zeroizing<String>,
	account_id: Zeroizing<String>,
	email: Zeroizing<String>,
	digest: String,
}

fn read_legacy_accounts(path: &Path) -> Result<Vec<LegacyCredential>, SupervisorError> {
	let (file, identity) = open_secure_regular(path, MAX_ACCOUNTS_BYTES)?;
	let mut bytes = Zeroizing::new(Vec::with_capacity(identity.length as usize));
	file.take(MAX_ACCOUNTS_BYTES + 1)
		.read_to_end(&mut bytes)
		.map_err(|_| SupervisorError::new("legacy accounts cannot be read"))?;
	if bytes.len() as u64 > MAX_ACCOUNTS_BYTES {
		return Err(SupervisorError::new("legacy accounts are too large"));
	}

	let mut accounts = Vec::new();
	for raw_line in bytes.split(|byte| *byte == b'\n') {
		let line = trim_ascii(raw_line);
		if line.is_empty() || line.starts_with(b"#") {
			continue;
		}
		if line.len() > MAX_LINE_BYTES || accounts.len() == MAX_ACCOUNTS {
			return Err(SupervisorError::new("legacy accounts exceed their bound"));
		}
		let parsed = serde_json::from_slice::<LegacyLineSerde>(line)
			.map_err(|_| SupervisorError::new("legacy accounts are malformed"))?;
		accounts.push(extract_legacy_credential(parsed.into())?);
	}
	if accounts.is_empty() {
		return Err(SupervisorError::new("legacy accounts are empty"));
	}

	let mut account_ids = BTreeSet::new();
	let mut emails = BTreeSet::new();
	for account in &accounts {
		let normalized_email = Zeroizing::new(account.email.to_lowercase());
		if !account_ids.insert(account.digest.clone())
			|| !emails.insert(hex_sha256(normalized_email.as_bytes()))
		{
			return Err(SupervisorError::new("legacy account identities are not unique"));
		}
	}

	Ok(accounts)
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
	while bytes.first().is_some_and(u8::is_ascii_whitespace) {
		bytes = &bytes[1..];
	}
	while bytes.last().is_some_and(u8::is_ascii_whitespace) {
		bytes = &bytes[..bytes.len() - 1];
	}

	bytes
}

fn extract_legacy_credential(mut line: LegacyLine) -> Result<LegacyCredential, SupervisorError> {
	if line.tokens.is_some() == line.auth.is_some() {
		return Err(SupervisorError::new("legacy account shape is invalid"));
	}
	let (auth_email, mut tokens) = if let Some(mut auth) = line.auth.take() {
		let email = auth.email.take();
		let tokens = auth
			.tokens
			.take()
			.ok_or_else(|| SupervisorError::new("legacy account tokens are missing"))?;

		(email, tokens)
	} else {
		(
			None,
			line.tokens
				.take()
				.ok_or_else(|| SupervisorError::new("legacy account tokens are missing"))?,
		)
	};
	let access_token = Zeroizing::new(std::mem::take(&mut tokens.access_token));
	if !valid_scalar(&access_token, MAX_ACCESS_TOKEN_BYTES) {
		return Err(SupervisorError::new("legacy access token is invalid"));
	}
	let id_token = Zeroizing::new(std::mem::take(&mut tokens.id_token));
	if !valid_scalar(&id_token, MAX_ID_TOKEN_BYTES) {
		return Err(SupervisorError::new("legacy identity token is invalid"));
	}
	let account_id = Zeroizing::new(
		tokens
			.account_id
			.take()
			.ok_or_else(|| SupervisorError::new("legacy provider account identity is missing"))?,
	);
	if !valid_scalar(&account_id, MAX_PROVIDER_ACCOUNT_ID_BYTES) {
		return Err(SupervisorError::new("legacy provider account identity is invalid"));
	}
	let email = Zeroizing::new(
		line.email
			.take()
			.or(auth_email)
			.ok_or_else(|| SupervisorError::new("legacy account email is missing"))?,
	);
	if !valid_scalar(&email, MAX_EMAIL_BYTES) || !email.contains('@') {
		return Err(SupervisorError::new("legacy account email is invalid"));
	}
	validate_identity_token(&id_token, &account_id, &email)?;

	let digest = hex_sha256(account_id.as_bytes());

	Ok(LegacyCredential { access_token, account_id, email, digest })
}

fn valid_scalar(value: &str, maximum_bytes: usize) -> bool {
	!value.is_empty()
		&& value.len() <= maximum_bytes
		&& value.trim() == value
		&& !value.chars().any(char::is_control)
}

fn validate_identity_token(
	token: &str,
	account_id: &str,
	email: &str,
) -> Result<(), SupervisorError> {
	let mut components = token.split('.');
	let header = components.next();
	let payload = components.next();
	let signature = components.next();
	if header.is_none_or(str::is_empty)
		|| payload.is_none_or(str::is_empty)
		|| signature.is_none_or(str::is_empty)
		|| components.next().is_some()
	{
		return Err(SupervisorError::new("legacy identity token is malformed"));
	}
	let payload = payload.expect("nonempty identity-token payload was checked");
	let decoded = if payload.ends_with('=') {
		URL_SAFE.decode(payload)
	} else {
		URL_SAFE_NO_PAD.decode(payload)
	}
	.map_err(|_| SupervisorError::new("legacy identity token is malformed"))?;
	let decoded = Zeroizing::new(decoded);
	let mut claims = serde_json::from_slice::<IdentityClaims>(&decoded)
		.map_err(|_| SupervisorError::new("legacy identity token is malformed"))?;
	let mut authority = claims
		.authority
		.take()
		.ok_or_else(|| SupervisorError::new("legacy identity token lacks account authority"))?;
	let claimed_account_id = Zeroizing::new(
		authority
			.chatgpt_account_id
			.take()
			.ok_or_else(|| SupervisorError::new("legacy identity claims are incomplete"))?,
	);
	let claimed_plan = Zeroizing::new(
		authority
			.chatgpt_plan_type
			.take()
			.ok_or_else(|| SupervisorError::new("legacy identity claims are incomplete"))?,
	);
	if !valid_scalar(&claimed_account_id, MAX_PROVIDER_ACCOUNT_ID_BYTES)
		|| claimed_account_id.as_str() != account_id
		|| !valid_scalar(&claimed_plan, MAX_PLAN_TYPE_BYTES)
		|| !valid_plan_type(&claimed_plan)
	{
		return Err(SupervisorError::new("legacy identity claims are inconsistent"));
	}
	if let Some(claimed_email) = claims.email.take() {
		let claimed_email = Zeroizing::new(claimed_email);
		if !valid_scalar(&claimed_email, MAX_EMAIL_BYTES) {
			return Err(SupervisorError::new("legacy identity email claim is invalid"));
		}
		let normalized_claim = Zeroizing::new(claimed_email.to_lowercase());
		let normalized_email = Zeroizing::new(email.to_lowercase());
		if normalized_claim != normalized_email {
			return Err(SupervisorError::new("legacy identity email claim is inconsistent"));
		}
	}

	Ok(())
}

fn valid_plan_type(value: &str) -> bool {
	matches!(
		value,
		"free"
			| "go" | "plus"
			| "pro" | "prolite"
			| "team" | "self_serve_business_usage_based"
			| "business"
			| "enterprise_cbp_usage_based"
			| "enterprise"
			| "edu" | "unknown"
	)
}

fn hex_sha256(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

struct SlotCredential {
	slot: u8,
	access_token: Zeroizing<String>,
	account_id: Zeroizing<String>,
	email: Zeroizing<String>,
}

fn credential_projection_fingerprint(credentials: &[SlotCredential]) -> Zeroizing<[u8; 32]> {
	let mut digest = Sha256::new();

	digest.update(CREDENTIAL_PROJECTION_FINGERPRINT_PROTOCOL);
	digest.update(
		u64::try_from(credentials.len())
			.expect("bounded credential count fits in u64")
			.to_be_bytes(),
	);
	for credential in credentials {
		update_length_delimited(&mut digest, &[credential.slot]);
		update_length_delimited(&mut digest, credential.access_token.as_bytes());
		update_length_delimited(&mut digest, credential.account_id.as_bytes());
		update_length_delimited(&mut digest, credential.email.as_bytes());
	}

	Zeroizing::new(digest.finalize().into())
}

fn update_length_delimited(digest: &mut Sha256, value: &[u8]) {
	digest.update(
		u64::try_from(value.len()).expect("bounded credential field fits in u64").to_be_bytes(),
	);
	digest.update(value);
}

fn map_credentials(
	mapping: Vec<MappingEntry>,
	accounts: Vec<LegacyCredential>,
) -> Result<Vec<SlotCredential>, SupervisorError> {
	if mapping.len() != accounts.len() {
		return Err(SupervisorError::new("credential mapping count does not match"));
	}
	let mut by_digest = accounts
		.into_iter()
		.map(|credential| (credential.digest.clone(), credential))
		.collect::<BTreeMap<_, _>>();
	let mut mapped = Vec::with_capacity(mapping.len());

	for entry in mapping {
		let credential = by_digest
			.remove(&entry.provider_account_id_sha256)
			.ok_or_else(|| SupervisorError::new("credential mapping does not match"))?;

		mapped.push(SlotCredential {
			slot: entry.slot,
			access_token: credential.access_token,
			account_id: credential.account_id,
			email: credential.email,
		});
	}
	if !by_digest.is_empty() {
		return Err(SupervisorError::new("credential mapping does not match"));
	}
	mapped.sort_by_key(|credential| credential.slot);

	Ok(mapped)
}

fn spawn_postgres(config: &LocalSupervisorConfig) -> Result<Child, SupervisorError> {
	let mut command = Command::new(&config.postgres);
	command
		.arg("-D")
		.arg(&config.data_directory)
		.arg("-k")
		.arg(&config.socket_directory)
		.arg("-p")
		.arg(config.port.to_string())
		.arg("-c")
		.arg("listen_addresses=")
		.current_dir(&config.working_directory)
		.stdin(Stdio::null())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit());
	scrub_slot_environment(&mut command);

	command.spawn().map_err(|_| SupervisorError::new("PostgreSQL cannot be started"))
}

async fn wait_for_postgres(
	config: &LocalSupervisorConfig,
	postgres: &mut Child,
	signals: &mut ShutdownSignals,
	socket_directory_identity: StableObjectIdentity,
) -> Result<PostgresStartup, SupervisorError> {
	let deadline = Instant::now() + STARTUP_TIMEOUT;

	loop {
		if child_exit(postgres)?.is_some() {
			return Err(SupervisorError::new("PostgreSQL exited before readiness"));
		}
		if secure_directory_identity(&config.socket_directory)? != socket_directory_identity {
			return Err(SupervisorError::new("PostgreSQL socket directory changed"));
		}
		if postgres_is_ready(config)? {
			let socket = socket_identity(&postgres_socket_path(config))?;
			let generation_path = config.data_directory.join("postmaster.pid");
			let generation =
				secure_regular_file_identity(&generation_path, 64 * 1024)?.stable_object();
			let process_id = read_postmaster_pid(&generation_path)?;
			if process_id != postgres.id() {
				return Err(SupervisorError::new("PostgreSQL generation is invalid"));
			}

			return Ok(PostgresStartup::Ready(PostgresIdentity {
				process_id,
				socket_directory: socket_directory_identity,
				socket,
				generation,
			}));
		}
		if Instant::now() >= deadline {
			return Err(SupervisorError::new("PostgreSQL readiness timed out"));
		}

		tokio::select! {
			signal = signals.recv() => {
				signal.map_err(|_| SupervisorError::new("supervisor signal handling failed"))?;
				return Ok(PostgresStartup::ShutdownRequested);
			},
			_ = tokio::time::sleep(POLL_INTERVAL) => {},
		}
	}
}

fn postgres_is_ready(config: &LocalSupervisorConfig) -> Result<bool, SupervisorError> {
	let mut command = Command::new(&config.pg_isready);
	command
		.arg("-q")
		.arg("-h")
		.arg(&config.socket_directory)
		.arg("-p")
		.arg(config.port.to_string())
		.arg("-d")
		.arg("postgres")
		.arg("-t")
		.arg("1")
		.current_dir(&config.working_directory)
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::null());
	scrub_slot_environment(&mut command);
	let status =
		command.status().map_err(|_| SupervisorError::new("PostgreSQL readiness probe failed"))?;

	Ok(status.success())
}

fn postgres_socket_path(config: &LocalSupervisorConfig) -> PathBuf {
	config.socket_directory.join(format!(".s.PGSQL.{}", config.port))
}

fn socket_identity(path: &Path) -> Result<StableObjectIdentity, SupervisorError> {
	let metadata = fs::symlink_metadata(path)
		.map_err(|_| SupervisorError::new("PostgreSQL socket is unavailable"))?;
	let identity = FileIdentity::from_metadata(&metadata)?;
	// SAFETY: `geteuid` has no arguments or failure return.
	let effective_uid = unsafe { libc::geteuid() };

	if metadata.file_type().is_symlink()
		|| identity.kind != FileKind::Socket
		|| identity.uid != effective_uid
	{
		return Err(SupervisorError::new("PostgreSQL socket is unsafe"));
	}

	Ok(identity.stable_object())
}

fn read_postmaster_pid(path: &Path) -> Result<u32, SupervisorError> {
	let (file, _) = open_secure_regular_relaxed_mode(path, 64 * 1024)?;
	let mut bytes = Vec::new();
	file.take(64 * 1024 + 1)
		.read_to_end(&mut bytes)
		.map_err(|_| SupervisorError::new("PostgreSQL generation cannot be read"))?;
	let first_line = bytes
		.split(|byte| *byte == b'\n')
		.next()
		.ok_or_else(|| SupervisorError::new("PostgreSQL generation is invalid"))?;
	let value = std::str::from_utf8(first_line)
		.map_err(|_| SupervisorError::new("PostgreSQL generation is invalid"))?
		.parse::<u32>()
		.map_err(|_| SupervisorError::new("PostgreSQL generation is invalid"))?;

	Ok(value)
}

fn open_secure_regular_relaxed_mode(
	path: &Path,
	maximum_bytes: u64,
) -> Result<(File, FileIdentity), SupervisorError> {
	let expected_metadata = fs::symlink_metadata(path)
		.map_err(|_| SupervisorError::new("PostgreSQL generation is unavailable"))?;
	let expected = FileIdentity::from_metadata(&expected_metadata)?;
	// SAFETY: `geteuid` has no arguments or failure return.
	let effective_uid = unsafe { libc::geteuid() };
	if expected_metadata.file_type().is_symlink()
		|| expected.kind != FileKind::Regular
		|| expected.uid != effective_uid
		|| expected.links != 1
		|| expected.length > maximum_bytes
	{
		return Err(SupervisorError::new("PostgreSQL generation is unsafe"));
	}
	let file = OpenOptions::new()
		.read(true)
		.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
		.open(path)
		.map_err(|_| SupervisorError::new("PostgreSQL generation cannot be opened"))?;
	let actual = FileIdentity::from_metadata(
		&file
			.metadata()
			.map_err(|_| SupervisorError::new("PostgreSQL generation cannot be inspected"))?,
	)?;
	if actual != expected {
		return Err(SupervisorError::new("PostgreSQL generation changed during open"));
	}

	Ok((file, actual))
}

fn spawn_daemon(
	config: &LocalSupervisorConfig,
	executable: &Path,
	credentials: &[SlotCredential],
) -> Result<Child, SupervisorError> {
	daemon_command(config, executable, credentials)
		.spawn()
		.map_err(|_| SupervisorError::new("decodexd child cannot be started"))
}

fn daemon_command(
	config: &LocalSupervisorConfig,
	executable: &Path,
	credentials: &[SlotCredential],
) -> Command {
	let mut command = Command::new(executable);
	command
		.arg("serve")
		.current_dir(&config.working_directory)
		.stdin(Stdio::null())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit());

	scrub_slot_environment(&mut command);
	for credential in credentials {
		command.env(
			slot_environment_name(credential.slot, "ACCESS_TOKEN"),
			credential.access_token.as_str(),
		);
		command.env(
			slot_environment_name(credential.slot, "ACCOUNT_ID"),
			credential.account_id.as_str(),
		);
		command.env(slot_environment_name(credential.slot, "EMAIL"), credential.email.as_str());
	}

	command
}

fn scrub_slot_environment(command: &mut Command) {
	for slot in 1..=MAX_ACCOUNTS {
		for suffix in ["ACCESS_TOKEN", "ACCOUNT_ID", "EMAIL"] {
			command.env_remove(slot_environment_name(slot as u8, suffix));
		}
	}
}

fn slot_environment_name(slot: u8, suffix: &str) -> OsString {
	OsString::from(format!("DECODEX_RESET_CARD_SLOT_{slot:02}_{suffix}"))
}

fn child_exit(child: &mut Child) -> Result<Option<ExitStatus>, SupervisorError> {
	child.try_wait().map_err(|_| SupervisorError::new("child process cannot be inspected"))
}

#[derive(Clone, Copy)]
enum ChildKind {
	Daemon,
	Postgres,
}

fn stop_child(child: &mut Child, kind: ChildKind) {
	if child.try_wait().ok().flatten().is_some() {
		return;
	}
	#[cfg(unix)]
	{
		// SAFETY: the PID came from a live `Child`; SIGTERM is a defined Unix signal.
		let _ = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
	}

	let deadline = Instant::now() + child_shutdown_timeout(kind);
	while Instant::now() < deadline {
		match child.try_wait() {
			Ok(Some(_)) => return,
			Ok(None) => thread::sleep(Duration::from_millis(20)),
			Err(_) => break,
		}
	}
	let _ = child.kill();
	let _ = child.wait();

	let message = match kind {
		ChildKind::Daemon => "decodexd child required forced termination",
		ChildKind::Postgres => "PostgreSQL child required forced termination",
	};
	let _ = writeln!(std::io::stderr().lock(), "{message}");
}

const fn child_shutdown_timeout(kind: ChildKind) -> Duration {
	match kind {
		ChildKind::Daemon => DAEMON_SHUTDOWN_TIMEOUT,
		ChildKind::Postgres => POSTGRES_SHUTDOWN_TIMEOUT,
	}
}

pub(super) struct SupervisorError {
	message: &'static str,
}

impl SupervisorError {
	const fn new(message: &'static str) -> Self {
		Self { message }
	}
}

impl Display for SupervisorError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(self.message)
	}
}

impl std::fmt::Debug for SupervisorError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(self.message)
	}
}

impl Error for SupervisorError {}

#[cfg(test)]
mod tests {
	use std::{
		collections::BTreeMap,
		ffi::OsString,
		fs,
		os::unix::fs::{PermissionsExt as _, symlink},
		path::{Path, PathBuf},
	};

	use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
	use tempfile::TempDir;
	use zeroize::Zeroizing;

	use super::{
		ChildKind, DAEMON_SHUTDOWN_TIMEOUT, LocalSupervisorConfig, MAPPING_SCHEMA,
		POSTGRES_SHUTDOWN_TIMEOUT, SlotCredential, WatchedCredentialFiles, account_lock_path,
		child_shutdown_timeout, credential_projection_fingerprint, daemon_command, hex_sha256,
		load_credentials, slot_environment_name,
	};

	#[test]
	fn secure_mapping_loads_exact_slots_without_retaining_unmapped_accounts() {
		let fixture = CredentialFixture::new();
		fixture.write_accounts(&[
			("first@example.test", "provider-first", "access-first"),
			("second@example.test", "provider-second", "access-second"),
		]);
		fixture.write_mapping(&[(2, "provider-second"), (1, "provider-first")]);

		let (loaded, _) =
			load_credentials(&fixture.accounts, &fixture.lock, &fixture.mapping).expect("load");

		assert_eq!(loaded.len(), 2);
		assert_eq!(loaded[0].slot, 1);
		assert_eq!(loaded[0].account_id.as_str(), "provider-first");
		assert_eq!(loaded[0].email.as_str(), "first@example.test");
		assert_eq!(loaded[0].access_token.as_str(), "access-first");
		assert_eq!(
			slot_environment_name(loaded[1].slot, "ACCESS_TOKEN"),
			"DECODEX_RESET_CARD_SLOT_02_ACCESS_TOKEN"
		);
	}

	#[test]
	fn mapping_requires_an_exact_legacy_account_count_and_digest_set() {
		let fixture = CredentialFixture::new();
		fixture.write_accounts(&[
			("first@example.test", "provider-first", "access-first"),
			("second@example.test", "provider-second", "access-second"),
		]);
		fixture.write_mapping(&[(1, "provider-first")]);

		assert!(load_credentials(&fixture.accounts, &fixture.lock, &fixture.mapping).is_err());

		fixture.write_mapping(&[(1, "provider-first"), (2, "wrong-provider")]);
		assert!(load_credentials(&fixture.accounts, &fixture.lock, &fixture.mapping).is_err());

		fixture.write_mapping(&[(1, "provider-first"), (3, "provider-second")]);
		assert!(load_credentials(&fixture.accounts, &fixture.lock, &fixture.mapping).is_err());
	}

	#[test]
	fn legacy_accounts_require_unique_provider_identity_and_email() {
		let fixture = CredentialFixture::new();
		fixture.write_accounts(&[
			("same@example.test", "provider-first", "access-first"),
			("SAME@example.test", "provider-second", "access-second"),
		]);
		fixture.write_mapping(&[(1, "provider-first"), (2, "provider-second")]);

		assert!(load_credentials(&fixture.accounts, &fixture.lock, &fixture.mapping).is_err());
	}

	#[test]
	fn legacy_account_reader_rejects_symlink_and_insecure_mode() {
		let fixture = CredentialFixture::new();
		fixture.write_accounts(&[("first@example.test", "provider-first", "access-first")]);
		fixture.write_mapping(&[(1, "provider-first")]);

		fs::set_permissions(&fixture.accounts, fs::Permissions::from_mode(0o644))
			.expect("make accounts unsafe");
		assert!(load_credentials(&fixture.accounts, &fixture.lock, &fixture.mapping).is_err());

		fs::set_permissions(&fixture.accounts, fs::Permissions::from_mode(0o600))
			.expect("restore accounts");
		let target = fixture.root.path().join("mapping-target.json");
		fs::rename(&fixture.mapping, &target).expect("move mapping");
		symlink(&target, &fixture.mapping).expect("symlink mapping");
		assert!(load_credentials(&fixture.accounts, &fixture.lock, &fixture.mapping).is_err());
	}

	#[test]
	fn legacy_account_reader_rejects_identity_claim_drift() {
		let fixture = CredentialFixture::new();
		fixture.write_mapping(&[(1, "provider-first")]);

		for (claimed_account, claimed_email, claimed_plan) in [
			("different-provider", "first@example.test", "pro"),
			("provider-first", "different@example.test", "pro"),
			("provider-first", "first@example.test", "private-plan"),
		] {
			let identity_token = identity_token(claimed_account, claimed_email, claimed_plan);
			secure_write(
				&fixture.accounts,
				&format!(
					r#"{{"email":"first@example.test","tokens":{{"id_token":"{identity_token}","access_token":"access-first","refresh_token":"not-read","account_id":"provider-first"}}}}
"#
				),
			);

			assert!(load_credentials(&fixture.accounts, &fixture.lock, &fixture.mapping).is_err());
		}
	}

	#[test]
	fn legacy_account_reader_rejects_scalar_whitespace_instead_of_normalizing_it() {
		let fixture = CredentialFixture::new();
		let identity_token = identity_token("provider-first", "first@example.test", "pro");
		secure_write(
			&fixture.accounts,
			&format!(
				r#"{{"email":"first@example.test","tokens":{{"id_token":"{identity_token}","access_token":" access-first","refresh_token":"not-read","account_id":"provider-first"}}}}
"#
			),
		);
		fixture.write_mapping(&[(1, "provider-first")]);

		assert!(load_credentials(&fixture.accounts, &fixture.lock, &fixture.mapping).is_err());
	}

	#[test]
	fn legacy_writer_lock_path_is_hidden_next_to_the_accounts_file() {
		assert_eq!(
			account_lock_path(Path::new("/tmp/pool/accounts.jsonl")).expect("derive lock"),
			PathBuf::from("/tmp/pool/.accounts.jsonl.lock")
		);
	}

	#[test]
	fn credential_watch_tracks_exact_files_without_restarting_for_unrelated_parent_entries() {
		let fixture = CredentialFixture::new();
		fixture.write_accounts(&[("first@example.test", "provider-first", "access-first")]);
		fixture.write_mapping(&[(1, "provider-first")]);
		let (_, loaded_watch) =
			load_credentials(&fixture.accounts, &fixture.lock, &fixture.mapping).expect("load");

		secure_write(
			&fixture.mapping.parent().expect("mapping parent").join("unrelated-state"),
			"unrelated",
		);
		let unrelated_watch =
			WatchedCredentialFiles::capture(&fixture.accounts, &fixture.lock, &fixture.mapping)
				.expect("capture unrelated parent change");
		assert!(loaded_watch == unrelated_watch);

		fs::set_permissions(&fixture.lock, fs::Permissions::from_mode(0o600))
			.expect("reapply secure lock mode");
		let metadata_only_lock_watch =
			WatchedCredentialFiles::capture(&fixture.accounts, &fixture.lock, &fixture.mapping)
				.expect("capture metadata-only lock change");
		assert!(loaded_watch == metadata_only_lock_watch);

		let replacement_lock = fixture.root.path().join("replacement.lock");
		secure_write(&replacement_lock, "");
		fs::rename(&replacement_lock, &fixture.lock).expect("replace lock inode");
		let replaced_lock_watch =
			WatchedCredentialFiles::capture(&fixture.accounts, &fixture.lock, &fixture.mapping)
				.expect("capture lock replacement");
		assert!(loaded_watch != replaced_lock_watch);
	}

	#[test]
	fn daemon_restart_requires_a_changed_credential_projection() {
		let fixture = CredentialFixture::new();
		let accounts = [("first@example.test", "provider-first", "access-first")];

		fixture.write_accounts(&accounts);
		fixture.write_mapping(&[(1, "provider-first")]);
		let (loaded, loaded_watch) =
			load_credentials(&fixture.accounts, &fixture.lock, &fixture.mapping).expect("load");
		let loaded_projection = credential_projection_fingerprint(&loaded);

		fixture.replace_accounts(&accounts, 2);
		let (same_credentials, same_credentials_watch) =
			load_credentials(&fixture.accounts, &fixture.lock, &fixture.mapping)
				.expect("reload same credentials");
		let same_projection = credential_projection_fingerprint(&same_credentials);

		assert!(loaded_watch != same_credentials_watch);
		assert!(loaded_projection.as_ref() == same_projection.as_ref());

		fixture.replace_accounts(&[("first@example.test", "provider-first", "access-second")], 3);
		let (changed_credentials, _) =
			load_credentials(&fixture.accounts, &fixture.lock, &fixture.mapping)
				.expect("reload changed credentials");
		let changed_projection = credential_projection_fingerprint(&changed_credentials);

		assert!(loaded_projection.as_ref() != changed_projection.as_ref());
	}

	#[test]
	fn daemon_command_injects_only_the_exact_mapped_slots() {
		let path = PathBuf::from("/unused");
		let config = LocalSupervisorConfig {
			postgres: path.clone(),
			pg_isready: path.clone(),
			data_directory: path.clone(),
			socket_directory: path.clone(),
			port: 1,
			legacy_accounts: path.clone(),
			legacy_mapping: path.clone(),
			working_directory: PathBuf::from("/"),
		};
		let credentials = [SlotCredential {
			slot: 1,
			access_token: Zeroizing::new(String::from("access-first")),
			account_id: Zeroizing::new(String::from("provider-first")),
			email: Zeroizing::new(String::from("first@example.test")),
		}];
		let command = daemon_command(&config, Path::new("/path/to/decodexd"), &credentials);
		let environments = command
			.get_envs()
			.map(|(name, value)| (name.to_owned(), value.map(OsString::from)))
			.collect::<BTreeMap<_, _>>();

		assert_eq!(command.get_program(), "/path/to/decodexd");
		assert_eq!(command.get_args().collect::<Vec<_>>(), ["serve"]);
		assert_eq!(
			environments.get(&OsString::from("DECODEX_RESET_CARD_SLOT_01_ACCESS_TOKEN")),
			Some(&Some(OsString::from("access-first")))
		);
		assert_eq!(
			environments.get(&OsString::from("DECODEX_RESET_CARD_SLOT_01_ACCOUNT_ID")),
			Some(&Some(OsString::from("provider-first")))
		);
		assert_eq!(
			environments.get(&OsString::from("DECODEX_RESET_CARD_SLOT_01_EMAIL")),
			Some(&Some(OsString::from("first@example.test")))
		);
		assert_eq!(
			environments.get(&OsString::from("DECODEX_RESET_CARD_SLOT_02_ACCESS_TOKEN")),
			Some(&None)
		);
	}

	#[test]
	fn child_shutdown_grace_preserves_bounded_provider_work_and_installer_drain_margin() {
		assert_eq!(child_shutdown_timeout(ChildKind::Daemon), DAEMON_SHUTDOWN_TIMEOUT);
		assert_eq!(DAEMON_SHUTDOWN_TIMEOUT.as_secs(), 240);
		assert!(DAEMON_SHUTDOWN_TIMEOUT > std::time::Duration::from_secs(212 + 5));
		assert_eq!(child_shutdown_timeout(ChildKind::Postgres), POSTGRES_SHUTDOWN_TIMEOUT);
		assert_eq!(POSTGRES_SHUTDOWN_TIMEOUT.as_secs(), 30);
		assert!(
			DAEMON_SHUTDOWN_TIMEOUT + POSTGRES_SHUTDOWN_TIMEOUT
				< std::time::Duration::from_secs(300)
		);
	}

	struct CredentialFixture {
		root: TempDir,
		accounts: PathBuf,
		lock: PathBuf,
		mapping: PathBuf,
	}

	impl CredentialFixture {
		fn new() -> Self {
			let root = TempDir::new().expect("create fixture");
			fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))
				.expect("secure fixture");
			let accounts = root.path().join("accounts.jsonl");
			let lock = root.path().join(".accounts.jsonl.lock");
			let mapping_parent = root.path().join("mapping");
			fs::create_dir(&mapping_parent).expect("create mapping parent");
			fs::set_permissions(&mapping_parent, fs::Permissions::from_mode(0o700))
				.expect("secure mapping parent");
			let mapping = mapping_parent.join("legacy.json");

			secure_write(&lock, "");

			Self { root, accounts, lock, mapping }
		}

		fn write_accounts(&self, accounts: &[(&str, &str, &str)]) {
			secure_write(&self.accounts, &accounts_body(accounts, 1));
		}

		fn replace_accounts(&self, accounts: &[(&str, &str, &str)], generation: u64) {
			let replacement = self.root.path().join("accounts-replacement.jsonl");

			secure_write(&replacement, &accounts_body(accounts, generation));
			fs::rename(replacement, &self.accounts).expect("replace accounts atomically");
		}

		fn write_mapping(&self, entries: &[(u8, &str)]) {
			let accounts = entries
				.iter()
				.map(|(slot, provider)| {
					format!(
						r#"{{"slot":{slot},"provider_account_id_sha256":"{}"}}"#,
						hex_sha256(provider.as_bytes())
					)
				})
				.collect::<Vec<_>>()
				.join(",");
			secure_write(
				&self.mapping,
				&format!(r#"{{"schema":"{MAPPING_SCHEMA}","accounts":[{accounts}]}}"#),
			);
		}
	}

	fn accounts_body(accounts: &[(&str, &str, &str)], generation: u64) -> String {
		let body = accounts
			.iter()
			.map(|(email, account_id, access_token)| {
				let identity_token = identity_token(account_id, email, "pro");

				format!(
					r#"{{"email":"{email}","last_selected_at_unix_epoch":{generation},"tokens":{{"id_token":"{identity_token}","access_token":"{access_token}","refresh_token":"not-read","account_id":"{account_id}"}}}}"#
				)
			})
			.collect::<Vec<_>>()
			.join("\n");

		body + "\n"
	}

	fn identity_token(account_id: &str, email: &str, plan_type: &str) -> String {
		let payload = format!(
			r#"{{"email":"{email}","https://api.openai.com/auth":{{"chatgpt_account_id":"{account_id}","chatgpt_plan_type":"{plan_type}"}}}}"#
		);

		format!("header.{}.signature", URL_SAFE_NO_PAD.encode(payload))
	}

	fn secure_write(path: &Path, body: &str) {
		fs::write(path, body).expect("write fixture");
		fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("secure fixture file");
	}
}
