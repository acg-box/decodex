//! Narrow versioned host credential storage for daemon-owned account lifecycle.

use std::{
	error::Error,
	fmt::{Debug, Display, Formatter},
};

use decodex_core::{
	AccountId, AccountOperationId, AccountProvider, CredentialBinding, CredentialFingerprint,
	CredentialStoreSchemaVersion, CredentialVersion, ProviderIdentity,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const FINGERPRINT_DOMAIN: &[u8] = b"decodex-host-credential-store-v1\0";

/// Secret bundle kept only in the host credential store and short-lived daemon memory.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct CredentialSecretBundle {
	access_token: String,
	refresh_token: String,
	id_token: Option<String>,
	plan_type: Option<String>,
	provider_email: String,
	token_type: String,
	access_token_expires_at_unix_micros: i64,
}
impl CredentialSecretBundle {
	/// Construct the complete ChatGPT bundle needed by Codex login and host refresh.
	pub fn chatgpt(
		access_token: String,
		refresh_token: String,
		id_token: Option<String>,
		plan_type: Option<String>,
		provider_email: String,
		token_type: String,
		access_token_expires_at_unix_micros: i64,
	) -> Result<Self, CredentialStoreError> {
		if access_token.is_empty()
			|| refresh_token.is_empty()
			|| provider_email.is_empty()
			|| provider_email.len() > 320
			|| provider_email.chars().any(char::is_control)
			|| !token_type.eq_ignore_ascii_case("bearer")
			|| access_token_expires_at_unix_micros <= 0
		{
			return Err(CredentialStoreError::InvalidBundle);
		}

		Ok(Self {
			access_token,
			refresh_token,
			id_token,
			plan_type,
			provider_email,
			token_type: "bearer".to_owned(),
			access_token_expires_at_unix_micros,
		})
	}

	/// Borrow the access token for one immediate Codex projection.
	pub fn access_token(&self) -> &str {
		&self.access_token
	}

	/// Borrow the refresh token for one serialized provider refresh.
	pub fn refresh_token(&self) -> &str {
		&self.refresh_token
	}

	/// Borrow the optional ID token.
	pub fn id_token(&self) -> Option<&str> {
		self.id_token.as_deref()
	}

	/// Borrow the non-secret plan hint carried with the secret bundle.
	pub fn plan_type(&self) -> Option<&str> {
		self.plan_type.as_deref()
	}

	/// Borrow the provider email used for exact post-login account readback.
	pub fn provider_email(&self) -> &str {
		&self.provider_email
	}

	/// Borrow the closed OAuth token type.
	pub fn token_type(&self) -> &str {
		&self.token_type
	}

	/// Return the exact access-token expiry in Unix microseconds.
	pub const fn access_token_expires_at_unix_micros(&self) -> i64 {
		self.access_token_expires_at_unix_micros
	}

	/// Compute the canonical non-secret binding before a cross-store effect begins.
	pub fn binding_for(
		&self,
		account_id: &AccountId,
		writer_operation_id: &AccountOperationId,
		version: CredentialVersion,
		provider: &ProviderIdentity,
	) -> Result<CredentialBinding, CredentialStoreError> {
		let persisted = PersistedCredentialV1::new(
			account_id,
			writer_operation_id,
			version,
			provider,
			self.clone(),
		);
		let bytes = encode(&persisted)?;

		persisted.binding(fingerprint(&bytes)?)
	}
}
impl Debug for CredentialSecretBundle {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("CredentialSecretBundle([REDACTED])")
	}
}

/// Exact host-store read containing secret material and its canonical non-secret binding.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct StoredCredential {
	#[zeroize(skip)]
	binding: CredentialBinding,
	bundle: CredentialSecretBundle,
}
impl StoredCredential {
	/// Borrow the canonical credential binding.
	pub fn binding(&self) -> &CredentialBinding {
		&self.binding
	}

	/// Borrow the secret bundle for one immediate daemon operation.
	pub fn bundle(&self) -> &CredentialSecretBundle {
		&self.bundle
	}

	/// Consume the read and return its secret bundle.
	pub fn into_bundle(mut self) -> CredentialSecretBundle {
		std::mem::replace(
			&mut self.bundle,
			CredentialSecretBundle {
				access_token: String::new(),
				refresh_token: String::new(),
				id_token: None,
				plan_type: None,
				provider_email: String::new(),
				token_type: String::new(),
				access_token_expires_at_unix_micros: 0,
			},
		)
	}
}
impl Debug for StoredCredential {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("StoredCredential")
			.field("binding", &self.binding)
			.field("bundle", &"[REDACTED]")
			.finish()
	}
}

/// Narrow host credential-store contract. Implementations must make each method atomic.
pub trait HostCredentialStore: Send + Sync {
	/// Create version one. Existing material is an exact typed conflict.
	fn create(
		&self,
		account_id: &AccountId,
		target: &CredentialBinding,
		bundle: CredentialSecretBundle,
	) -> Result<(), CredentialStoreError>;

	/// Read only when schema, version, fingerprint, and provider all agree.
	fn read_exact(
		&self,
		account_id: &AccountId,
		expected: &CredentialBinding,
	) -> Result<StoredCredential, CredentialStoreError>;

	/// Rotate only from the exact expected binding to its immediate successor.
	fn compare_and_swap_rotate(
		&self,
		account_id: &AccountId,
		expected: &CredentialBinding,
		target: &CredentialBinding,
		bundle: CredentialSecretBundle,
	) -> Result<(), CredentialStoreError>;

	/// Delete only the exact expected version and fingerprint.
	fn delete(
		&self,
		account_id: &AccountId,
		expected: &CredentialBinding,
	) -> Result<(), CredentialStoreError>;
}

/// Closed store failure that cannot carry credential material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialStoreError {
	/// The protected store or its serialization boundary is unavailable.
	Unavailable,
	/// No exact account item exists.
	NotFound,
	/// Create found an existing account item.
	AlreadyExists,
	/// The current or target credential version is incompatible.
	VersionConflict,
	/// The current serialized bundle digest differs.
	FingerprintMismatch,
	/// The current provider identity differs.
	ProviderMismatch,
	/// The serialized account identity differs.
	AccountMismatch,
	/// The current writer operation differs.
	WriterMismatch,
	/// The serialized store schema is not supported.
	UnsupportedSchema,
	/// A caller supplied an invalid secret bundle.
	InvalidBundle,
	/// A stored bundle is malformed or internally inconsistent.
	CorruptBundle,
}
impl Error for CredentialStoreError {}
impl Display for CredentialStoreError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::Unavailable => "host credential store unavailable",
			Self::NotFound => "host credential item not found",
			Self::AlreadyExists => "host credential item already exists",
			Self::VersionConflict => "host credential version conflict",
			Self::FingerprintMismatch => "host credential fingerprint mismatch",
			Self::ProviderMismatch => "host credential provider mismatch",
			Self::AccountMismatch => "host credential account mismatch",
			Self::WriterMismatch => "host credential writer operation mismatch",
			Self::UnsupportedSchema => "host credential schema unsupported",
			Self::InvalidBundle => "host credential bundle invalid",
			Self::CorruptBundle => "host credential bundle corrupt",
		})
	}
}

#[derive(Deserialize, Serialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct PersistedCredentialV1 {
	schema_version: u16,
	account_id: String,
	credential_version: u64,
	writer_operation_id: String,
	provider: String,
	provider_account_id: String,
	access_token: String,
	refresh_token: String,
	id_token: Option<String>,
	plan_type: Option<String>,
	provider_email: String,
	token_type: String,
	access_token_expires_at_unix_micros: i64,
}
impl PersistedCredentialV1 {
	fn new(
		account_id: &AccountId,
		writer_operation_id: &AccountOperationId,
		version: CredentialVersion,
		provider: &ProviderIdentity,
		mut bundle: CredentialSecretBundle,
	) -> Self {
		Self {
			schema_version: CredentialStoreSchemaVersion::V1.get(),
			account_id: account_id.as_str().to_owned(),
			credential_version: version.get(),
			writer_operation_id: writer_operation_id.as_str().to_owned(),
			provider: provider_text(provider.provider()).to_owned(),
			provider_account_id: provider.account_id().to_owned(),
			access_token: std::mem::take(&mut bundle.access_token),
			refresh_token: std::mem::take(&mut bundle.refresh_token),
			id_token: bundle.id_token.take(),
			plan_type: bundle.plan_type.take(),
			provider_email: std::mem::take(&mut bundle.provider_email),
			token_type: std::mem::take(&mut bundle.token_type),
			access_token_expires_at_unix_micros: bundle.access_token_expires_at_unix_micros,
		}
	}

	fn binding(
		&self,
		fingerprint: CredentialFingerprint,
	) -> Result<CredentialBinding, CredentialStoreError> {
		let schema_version = CredentialStoreSchemaVersion::new(self.schema_version)
			.map_err(|_| CredentialStoreError::UnsupportedSchema)?;
		let version = CredentialVersion::new(self.credential_version)
			.map_err(|_| CredentialStoreError::CorruptBundle)?;
		let provider_kind = match self.provider.as_str() {
			"chatgpt" => AccountProvider::Chatgpt,
			_ => return Err(CredentialStoreError::CorruptBundle),
		};
		let provider = ProviderIdentity::new(provider_kind, self.provider_account_id.clone())
			.map_err(|_| CredentialStoreError::CorruptBundle)?;
		let writer_operation_id = AccountOperationId::new(self.writer_operation_id.clone())
			.map_err(|_| CredentialStoreError::CorruptBundle)?;

		Ok(CredentialBinding {
			schema_version,
			version,
			fingerprint,
			provider,
			writer_operation_id,
		})
	}

	fn into_bundle(mut self) -> Result<CredentialSecretBundle, CredentialStoreError> {
		CredentialSecretBundle::chatgpt(
			std::mem::take(&mut self.access_token),
			std::mem::take(&mut self.refresh_token),
			self.id_token.take(),
			self.plan_type.take(),
			std::mem::take(&mut self.provider_email),
			std::mem::take(&mut self.token_type),
			self.access_token_expires_at_unix_micros,
		)
	}

	fn account_id(&self) -> Result<AccountId, CredentialStoreError> {
		AccountId::new(self.account_id.clone()).map_err(|_| CredentialStoreError::CorruptBundle)
	}
}

fn encode(persisted: &PersistedCredentialV1) -> Result<Zeroizing<Vec<u8>>, CredentialStoreError> {
	serde_json::to_vec(persisted)
		.map(Zeroizing::new)
		.map_err(|_| CredentialStoreError::InvalidBundle)
}

fn decode(
	bytes: Vec<u8>,
) -> Result<(PersistedCredentialV1, CredentialFingerprint), CredentialStoreError> {
	let bytes = Zeroizing::new(bytes);
	let fingerprint = fingerprint(&bytes)?;
	let persisted =
		serde_json::from_slice(&bytes).map_err(|_| CredentialStoreError::CorruptBundle)?;

	Ok((persisted, fingerprint))
}

fn fingerprint(bytes: &[u8]) -> Result<CredentialFingerprint, CredentialStoreError> {
	let mut digest = Sha256::new();
	digest.update(FINGERPRINT_DOMAIN);
	digest.update(bytes);
	CredentialFingerprint::new(
		digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
	)
	.map_err(|_| CredentialStoreError::CorruptBundle)
}

const fn provider_text(provider: AccountProvider) -> &'static str {
	match provider {
		AccountProvider::Chatgpt => "chatgpt",
	}
}

fn enforce_exact(
	actual: &CredentialBinding,
	expected: &CredentialBinding,
) -> Result<(), CredentialStoreError> {
	if actual.schema_version != expected.schema_version {
		return Err(CredentialStoreError::UnsupportedSchema);
	}
	if actual.version != expected.version {
		return Err(CredentialStoreError::VersionConflict);
	}
	if actual.fingerprint != expected.fingerprint {
		return Err(CredentialStoreError::FingerprintMismatch);
	}
	if actual.provider != expected.provider {
		return Err(CredentialStoreError::ProviderMismatch);
	}
	if actual.writer_operation_id != expected.writer_operation_id {
		return Err(CredentialStoreError::WriterMismatch);
	}

	Ok(())
}

/// Seal one exact host-store read after canonical reconstruction and typed comparison.
pub(crate) fn seal_exact_read(
	account_id: &AccountId,
	actual: &CredentialBinding,
	expected: &CredentialBinding,
	bundle: CredentialSecretBundle,
) -> Result<StoredCredential, CredentialStoreError> {
	let recomputed = bundle.binding_for(
		account_id,
		&actual.writer_operation_id,
		actual.version,
		&actual.provider,
	)?;
	enforce_exact(&recomputed, actual)?;
	enforce_exact(actual, expected)?;

	Ok(StoredCredential { binding: actual.clone(), bundle })
}

#[cfg(target_os = "macos")]
mod macos {
	use std::{
		fs::{File, OpenOptions},
		os::{
			fd::AsRawFd as _,
			unix::fs::{MetadataExt as _, OpenOptionsExt as _},
		},
		path::PathBuf,
		sync::Mutex,
	};

	use crate::daemon_wrapper::inspect_current_daemon_wrapper;
	use decodex_core::DecodexPaths;
	use security_framework::{
		access_control::{ProtectionMode, SecAccessControl},
		passwords::{
			PasswordOptions, delete_generic_password_options, generic_password,
			set_generic_password_options,
		},
	};

	use super::{
		AccountId, CredentialBinding, CredentialSecretBundle, CredentialStoreError,
		CredentialVersion, HostCredentialStore, PersistedCredentialV1, StoredCredential, decode,
		encode, enforce_exact, fingerprint, seal_exact_read,
	};

	const APPLICATION_IDENTITY: &str = "box.acg.decodex";
	const KEYCHAIN_SERVICE: &str = "box.acg.decodex.credentials.v1";
	const KEYCHAIN_ACCESS_GROUP: &str = "T54QFA7W2S.box.acg.decodex.daemon";
	const ITEM_NOT_FOUND: i32 = -25_300;

	#[derive(Debug, Eq, PartialEq)]
	struct KeychainQueryAuthority {
		service: &'static str,
		account: String,
		access_group: String,
	}

	/// macOS generic-password adapter used only by the singleton daemon Account Service.
	pub struct MacosKeychainCredentialStore {
		serial: Mutex<()>,
		lock_path: PathBuf,
		access_group: String,
	}
	impl MacosKeychainCredentialStore {
		/// Construct the daemon-owned Keychain adapter from the verified current wrapper.
		pub fn new(paths: &DecodexPaths) -> Result<Self, CredentialStoreError> {
			let descriptor =
				inspect_current_daemon_wrapper().map_err(|_| CredentialStoreError::Unavailable)?;
			let access_group = descriptor.keychain_access_group();
			if access_group != KEYCHAIN_ACCESS_GROUP {
				return Err(CredentialStoreError::Unavailable);
			}
			Ok(Self {
				serial: Mutex::new(()),
				lock_path: paths.server_dir().join("account-credential-store.lock"),
				access_group: access_group.to_owned(),
			})
		}

		#[cfg(test)]
		fn new_for_process_lock_test(paths: &DecodexPaths) -> Self {
			Self {
				serial: Mutex::new(()),
				lock_path: paths.server_dir().join("account-credential-store.lock"),
				access_group: KEYCHAIN_ACCESS_GROUP.to_owned(),
			}
		}

		fn lock_process(&self) -> Result<ProcessCredentialLock, CredentialStoreError> {
			let file = OpenOptions::new()
				.read(true)
				.write(true)
				.create(true)
				.mode(0o600)
				.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
				.open(&self.lock_path)
				.map_err(|_| CredentialStoreError::Unavailable)?;
			let metadata = file.metadata().map_err(|_| CredentialStoreError::Unavailable)?;
			if !metadata.file_type().is_file()
				|| metadata.uid() != unsafe { libc::geteuid() }
				|| metadata.mode() & 0o077 != 0
				|| metadata.nlink() != 1
			{
				return Err(CredentialStoreError::Unavailable);
			}
			if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
				return Err(CredentialStoreError::Unavailable);
			}
			Ok(ProcessCredentialLock(file))
		}

		fn query_authority(&self, account_id: &AccountId) -> KeychainQueryAuthority {
			KeychainQueryAuthority {
				service: KEYCHAIN_SERVICE,
				account: account_id.as_str().to_owned(),
				access_group: self.access_group.clone(),
			}
		}

		fn query(&self, account_id: &AccountId) -> PasswordOptions {
			let authority = self.query_authority(account_id);
			let mut options =
				PasswordOptions::new_generic_password(authority.service, &authority.account);
			options.set_access_group(&authority.access_group);
			options.set_access_synchronized(Some(false));
			options.use_protected_keychain();
			options
		}

		fn write_options(
			&self,
			account_id: &AccountId,
		) -> Result<PasswordOptions, CredentialStoreError> {
			let mut options = self.query(account_id);
			let access = SecAccessControl::create_with_protection(
				Some(ProtectionMode::AccessibleAfterFirstUnlockThisDeviceOnly),
				0,
			)
			.map_err(|_| CredentialStoreError::Unavailable)?;
			options.set_access_control(access);
			options.set_label(APPLICATION_IDENTITY);
			options.set_description("Decodex daemon account credential bundle");
			Ok(options)
		}

		fn read_unlocked(
			&self,
			account_id: &AccountId,
		) -> Result<(PersistedCredentialV1, CredentialBinding), CredentialStoreError> {
			let bytes = generic_password(self.query(account_id)).map_err(map_keychain_error)?;
			let (persisted, fingerprint) = decode(bytes)?;
			if persisted.account_id()? != *account_id {
				return Err(CredentialStoreError::AccountMismatch);
			}
			let binding = persisted.binding(fingerprint)?;

			Ok((persisted, binding))
		}

		#[cfg(test)]
		fn enforce_metadata_access_group(
			&self,
			observed: Option<&str>,
		) -> Result<(), CredentialStoreError> {
			if observed == Some(self.access_group.as_str()) {
				Ok(())
			} else {
				Err(CredentialStoreError::CorruptBundle)
			}
		}
	}

	struct ProcessCredentialLock(File);
	impl Drop for ProcessCredentialLock {
		fn drop(&mut self) {
			let _ = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
		}
	}
	impl HostCredentialStore for MacosKeychainCredentialStore {
		fn create(
			&self,
			account_id: &AccountId,
			target: &CredentialBinding,
			bundle: CredentialSecretBundle,
		) -> Result<(), CredentialStoreError> {
			let _guard = self.serial.lock().map_err(|_| CredentialStoreError::Unavailable)?;
			let _process_guard = self.lock_process()?;
			if target.version.get() != 1 {
				return Err(CredentialStoreError::VersionConflict);
			}
			match self.read_unlocked(account_id) {
				Ok(_) => return Err(CredentialStoreError::AlreadyExists),
				Err(CredentialStoreError::NotFound) => {},
				Err(error) => return Err(error),
			}
			let persisted = PersistedCredentialV1::new(
				account_id,
				&target.writer_operation_id,
				CredentialVersion::new(1).map_err(|_| CredentialStoreError::InvalidBundle)?,
				&target.provider,
				bundle,
			);
			let bytes = encode(&persisted)?;
			let binding = persisted.binding(fingerprint(&bytes)?)?;
			enforce_exact(&binding, target)?;
			set_generic_password_options(&bytes, self.write_options(account_id)?)
				.map_err(map_keychain_error)?;

			Ok(())
		}

		fn read_exact(
			&self,
			account_id: &AccountId,
			expected: &CredentialBinding,
		) -> Result<StoredCredential, CredentialStoreError> {
			let _guard = self.serial.lock().map_err(|_| CredentialStoreError::Unavailable)?;
			let _process_guard = self.lock_process()?;
			let (persisted, binding) = self.read_unlocked(account_id)?;
			let bundle = persisted.into_bundle()?;

			seal_exact_read(account_id, &binding, expected, bundle)
		}

		fn compare_and_swap_rotate(
			&self,
			account_id: &AccountId,
			expected: &CredentialBinding,
			target: &CredentialBinding,
			bundle: CredentialSecretBundle,
		) -> Result<(), CredentialStoreError> {
			let _guard = self.serial.lock().map_err(|_| CredentialStoreError::Unavailable)?;
			let _process_guard = self.lock_process()?;
			let (_, actual) = self.read_unlocked(account_id)?;
			enforce_exact(&actual, expected)?;
			let next =
				expected.version.successor().map_err(|_| CredentialStoreError::VersionConflict)?;
			if target.version != next || target.provider != expected.provider {
				return Err(CredentialStoreError::VersionConflict);
			}
			let persisted = PersistedCredentialV1::new(
				account_id,
				&target.writer_operation_id,
				next,
				&target.provider,
				bundle,
			);
			let bytes = encode(&persisted)?;
			let binding = persisted.binding(fingerprint(&bytes)?)?;
			enforce_exact(&binding, target)?;
			set_generic_password_options(&bytes, self.write_options(account_id)?)
				.map_err(map_keychain_error)?;

			Ok(())
		}

		fn delete(
			&self,
			account_id: &AccountId,
			expected: &CredentialBinding,
		) -> Result<(), CredentialStoreError> {
			let _guard = self.serial.lock().map_err(|_| CredentialStoreError::Unavailable)?;
			let _process_guard = self.lock_process()?;
			let (_, actual) = self.read_unlocked(account_id)?;
			enforce_exact(&actual, expected)?;
			delete_generic_password_options(self.query(account_id)).map_err(map_keychain_error)
		}
	}

	fn map_keychain_error(error: security_framework::base::Error) -> CredentialStoreError {
		if error.code() == ITEM_NOT_FOUND {
			CredentialStoreError::NotFound
		} else {
			CredentialStoreError::Unavailable
		}
	}

	#[cfg(test)]
	mod tests {
		use std::{
			env, fs,
			process::{Command, Stdio},
			thread,
			time::{Duration, Instant},
		};

		use super::{KEYCHAIN_ACCESS_GROUP, KEYCHAIN_SERVICE, MacosKeychainCredentialStore};
		use crate::CredentialStoreError;
		use decodex_core::{AccountId, DecodexRoot};

		const CHILD_ROOT_ENV: &str = "DECODEX_TEST_KEYCHAIN_LOCK_CHILD_ROOT";
		const TEST_NAME: &str =
			"host_credentials::macos::tests::keychain_cas_boundary_blocks_competing_writer_process";

		#[test]
		fn keychain_cas_boundary_blocks_competing_writer_process() {
			if let Some(root) = env::var_os(CHILD_ROOT_ENV) {
				let root = DecodexRoot::new(root).expect("child root is valid");
				let paths = root.paths();
				fs::write(root.as_path().join("child-lock-attempt"), b"ready")
					.expect("child attempt marker is writable");
				let store = MacosKeychainCredentialStore::new_for_process_lock_test(&paths);
				let _guard =
					store.lock_process().expect("child acquires the released process lock");
				return;
			}

			let temporary_root =
				fs::canonicalize(env::temp_dir()).expect("temporary root is canonical");
			let temporary = tempfile::Builder::new()
				.prefix("decodex-keychain-lock-")
				.tempdir_in(temporary_root)
				.expect("temporary root exists");
			let root = DecodexRoot::new(temporary.path().join("decodex-root"))
				.expect("temporary Decodex root is valid");
			let paths = root.paths();
			paths.ensure_local_transport_layout().expect("private server directory exists");
			let store = MacosKeychainCredentialStore::new_for_process_lock_test(&paths);
			let account_id = AccountId::new("00000000-0000-4000-8000-000000000001".to_owned())
				.expect("test account identity is valid");
			let authority = store.query_authority(&account_id);
			assert_eq!(authority.service, KEYCHAIN_SERVICE);
			assert_eq!(authority.account, account_id.as_str());
			assert_eq!(authority.access_group, KEYCHAIN_ACCESS_GROUP);
			assert_eq!(store.enforce_metadata_access_group(Some(KEYCHAIN_ACCESS_GROUP)), Ok(()));
			assert_eq!(
				store.enforce_metadata_access_group(None),
				Err(CredentialStoreError::CorruptBundle)
			);
			assert_eq!(
				store.enforce_metadata_access_group(Some("wrong.group")),
				Err(CredentialStoreError::CorruptBundle)
			);
			let guard = store.lock_process().expect("parent acquires the process lock");
			let mut child = Command::new(env::current_exe().expect("test executable is available"))
				.args(["--exact", TEST_NAME])
				.env(CHILD_ROOT_ENV, root.as_path())
				.stdin(Stdio::null())
				.stdout(Stdio::null())
				.stderr(Stdio::null())
				.spawn()
				.expect("competing writer process starts");
			let marker = root.as_path().join("child-lock-attempt");
			let marker_deadline = Instant::now() + Duration::from_secs(5);
			while !marker.exists() && Instant::now() < marker_deadline {
				thread::sleep(Duration::from_millis(10));
			}
			assert!(marker.exists(), "competing writer reached the process-lock boundary");
			thread::sleep(Duration::from_millis(50));
			assert!(
				child.try_wait().expect("competing writer state is observable").is_none(),
				"competing writer must remain blocked while the first writer owns the boundary",
			);

			drop(guard);
			let completion_deadline = Instant::now() + Duration::from_secs(5);
			loop {
				if let Some(status) =
					child.try_wait().expect("competing writer state is observable")
				{
					assert!(status.success(), "competing writer acquires the released boundary");
					break;
				}
				assert!(
					Instant::now() < completion_deadline,
					"competing writer did not acquire the released boundary",
				);
				thread::sleep(Duration::from_millis(10));
			}
		}
	}
}

#[cfg(target_os = "macos")] pub use macos::MacosKeychainCredentialStore;
