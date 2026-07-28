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
#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
use std::path::Path;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const FINGERPRINT_DOMAIN: &[u8] = b"decodex-host-credential-store-v1\0";

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
const KEYCHAIN_ACCESSIBILITY_AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY: &str = "cku";

/// Non-secret readback from the production store for one finite canonical migration-gate slot.
#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AccountMigrationCredentialReadback {
	/// Stable report schema.
	pub schema: &'static str,
	/// Whether the exact protected item is present.
	pub present: bool,
	/// Production generic-password service.
	pub service: &'static str,
	/// Exact Keychain account name.
	pub account: String,
	/// Decoded store schema when present.
	pub store_schema_version: Option<u16>,
	/// Decoded provider kind when present.
	pub provider: Option<&'static str>,
	/// Decoded provider identity when present.
	pub provider_account_id: Option<String>,
	/// Decoded credential version when present.
	pub credential_version: Option<u64>,
	/// Decoded writer operation when present.
	pub writer_operation_id: Option<String>,
	/// Domain-separated complete-bundle fingerprint when present.
	pub fingerprint_sha256: Option<String>,
	/// Exact Keychain item label when present.
	pub label: Option<String>,
	/// Exact verified Keychain access group when present.
	pub access_group: Option<String>,
	/// Exact Keychain item description when present.
	pub description: Option<String>,
	/// Keychain accessibility code returned with the item attributes.
	pub accessibility: Option<String>,
	/// Whether the item is synchronizing.
	pub synchronizing: Option<bool>,
	/// Whether Keychain returned the access-control attribute.
	pub access_control_present: Option<bool>,
	/// Whether the query was pinned to the protected data keychain.
	pub protected_keychain: bool,
}

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
	#[cfg(feature = "account-migration-transition-gate")]
	use core_foundation::{
		base::{CFType, CFTypeRef, TCFType as _},
		boolean::CFBoolean,
		dictionary::CFDictionary,
		number::CFNumber,
		string::CFString,
	};
	use decodex_core::DecodexPaths;
	#[cfg(feature = "account-migration-transition-gate")]
	use security_framework::item::{ItemClass, ItemSearchOptions, Limit, SearchResult};
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
		encode, enforce_exact, fingerprint, provider_text,
	};
	#[cfg(feature = "account-migration-transition-gate")]
	use super::{
		AccountMigrationCredentialReadback,
		KEYCHAIN_ACCESSIBILITY_AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY,
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

		#[cfg(any(test, feature = "account-migration-transition-gate"))]
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

		#[cfg(feature = "account-migration-transition-gate")]
		pub(super) fn gate_metadata(
			&self,
			account_id: &AccountId,
		) -> Result<AccountMigrationCredentialReadback, CredentialStoreError> {
			let _guard = self.serial.lock().map_err(|_| CredentialStoreError::Unavailable)?;
			let _process_guard = self.lock_process()?;
			let (_persisted, binding) = match self.read_unlocked(account_id) {
				Ok(value) => value,
				Err(CredentialStoreError::NotFound) =>
					return Ok(AccountMigrationCredentialReadback {
						schema: "decodex/account-migration-credential-gate-readback/1",
						present: false,
						service: KEYCHAIN_SERVICE,
						account: account_id.as_str().to_owned(),
						store_schema_version: None,
						provider: None,
						provider_account_id: None,
						credential_version: None,
						writer_operation_id: None,
						fingerprint_sha256: None,
						label: None,
						access_group: None,
						description: None,
						accessibility: None,
						synchronizing: None,
						access_control_present: None,
						protected_keychain: true,
					}),
				Err(error) => return Err(error),
			};
			let attributes = keychain_attributes(&self.access_group, account_id)?;
			if self.enforce_metadata_access_group(attributes.access_group.as_deref()).is_err()
				|| attributes.service.as_deref() != Some(KEYCHAIN_SERVICE)
				|| attributes.account.as_deref() != Some(account_id.as_str())
				|| attributes.label.as_deref() != Some(APPLICATION_IDENTITY)
				|| attributes.description.as_deref()
					!= Some("Decodex daemon account credential bundle")
				|| attributes.accessibility.as_deref()
					!= Some(KEYCHAIN_ACCESSIBILITY_AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY)
				|| attributes.synchronizing != Some(false)
				|| !attributes.access_control_present
			{
				return Err(CredentialStoreError::CorruptBundle);
			}

			Ok(AccountMigrationCredentialReadback {
				schema: "decodex/account-migration-credential-gate-readback/1",
				present: true,
				service: KEYCHAIN_SERVICE,
				account: account_id.as_str().to_owned(),
				store_schema_version: Some(binding.schema_version.get()),
				provider: Some(provider_text(binding.provider.provider())),
				provider_account_id: Some(binding.provider.account_id().to_owned()),
				credential_version: Some(binding.version.get()),
				writer_operation_id: Some(binding.writer_operation_id.as_str().to_owned()),
				fingerprint_sha256: Some(binding.fingerprint.as_str().to_owned()),
				label: attributes.label,
				access_group: attributes.access_group,
				description: attributes.description,
				accessibility: attributes.accessibility,
				synchronizing: attributes.synchronizing,
				access_control_present: Some(attributes.access_control_present),
				protected_keychain: true,
			})
		}
	}

	#[cfg(feature = "account-migration-transition-gate")]
	struct KeychainAttributes {
		service: Option<String>,
		account: Option<String>,
		label: Option<String>,
		access_group: Option<String>,
		description: Option<String>,
		accessibility: Option<String>,
		synchronizing: Option<bool>,
		access_control_present: bool,
	}

	#[cfg(feature = "account-migration-transition-gate")]
	fn keychain_attributes(
		access_group: &str,
		account_id: &AccountId,
	) -> Result<KeychainAttributes, CredentialStoreError> {
		let mut query = ItemSearchOptions::new();
		query
			.class(ItemClass::generic_password())
			.service(KEYCHAIN_SERVICE)
			.account(account_id.as_str())
			.access_group(access_group)
			.cloud_sync(Some(false))
			.ignore_legacy_keychains()
			.load_attributes(true)
			.limit(Limit::Max(2));
		let mut results = query.search().map_err(map_keychain_error)?;
		if results.len() != 1 {
			return Err(CredentialStoreError::CorruptBundle);
		}
		let SearchResult::Dict(attributes) = results.remove(0) else {
			return Err(CredentialStoreError::CorruptBundle);
		};
		Ok(KeychainAttributes {
			service: attribute_string(&attributes, "svce"),
			account: attribute_string(&attributes, "acct"),
			label: attribute_string(&attributes, "labl"),
			access_group: attribute_string(&attributes, "agrp"),
			description: attribute_string(&attributes, "desc"),
			accessibility: attribute_string(&attributes, "pdmn"),
			synchronizing: attribute_bool(&attributes, "sync"),
			access_control_present: attribute(&attributes, "accc").is_some(),
		})
	}

	#[cfg(feature = "account-migration-transition-gate")]
	fn attribute(attributes: &CFDictionary, name: &str) -> Option<CFType> {
		let (keys, values) = attributes.get_keys_and_values();
		for (key, value) in keys.into_iter().zip(values) {
			// SAFETY: the retained dictionary owns each non-null key for this loop.
			let key = unsafe { CFType::wrap_under_get_rule(key as CFTypeRef) };
			if key.downcast::<CFString>().is_some_and(|key| key == name) {
				// SAFETY: the retained dictionary owns the matching non-null value.
				return Some(unsafe { CFType::wrap_under_get_rule(value as CFTypeRef) });
			}
		}
		None
	}

	#[cfg(feature = "account-migration-transition-gate")]
	fn attribute_string(attributes: &CFDictionary, name: &str) -> Option<String> {
		attribute(attributes, name)?.downcast::<CFString>().map(|value| value.to_string())
	}

	#[cfg(feature = "account-migration-transition-gate")]
	fn attribute_bool(attributes: &CFDictionary, name: &str) -> Option<bool> {
		let value = attribute(attributes, name)?;
		if let Some(value) = value.downcast::<CFBoolean>() {
			return Some(bool::from(value));
		}
		value.downcast::<CFNumber>()?.to_i32().map(|value| value != 0)
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
			enforce_exact(&binding, expected)?;
			let bundle = persisted.into_bundle()?;

			Ok(StoredCredential { binding, bundle })
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

			let temporary = tempfile::Builder::new()
				.prefix("decodex-keychain-lock-")
				.tempdir_in("/private/tmp")
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

/// Execute one finite protected-store operation for the canonical account-migration gate.
#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
pub fn run_account_migration_credential_gate(
	run_descriptor: &Path,
	action: &str,
	slot: Option<&str>,
) -> Result<Value, CredentialStoreError> {
	let run = crate::account_migration::load_account_migration_gate_run(run_descriptor)
		.map_err(|_| CredentialStoreError::InvalidBundle)?;
	match (action, slot) {
		("readback", Some(slot)) => {
			let account_id = gate_slot_account_id(&run.run_id, slot)?;
			let metadata =
				MacosKeychainCredentialStore::new(&run.paths)?.gate_metadata(&account_id)?;
			serde_json::to_value(metadata).map_err(|_| CredentialStoreError::Unavailable)
		},
		("prove_create_conflict", None) => prove_gate_create_conflict(&run),
		("cleanup_run", None) => cleanup_gate_run_credentials(&run),
		_ => Err(CredentialStoreError::InvalidBundle),
	}
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
fn gate_slot_account_id(run_id: &str, slot: &str) -> Result<AccountId, CredentialStoreError> {
	if !matches!(slot, "account_1" | "account_2" | "account_3" | "account_4" | "conflict") {
		return Err(CredentialStoreError::InvalidBundle);
	}
	AccountId::new(crate::account_migration::account_migration_gate_uuid(run_id, slot, "account"))
		.map_err(|_| CredentialStoreError::InvalidBundle)
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
fn gate_operation_id(run_id: &str, slot: &str) -> Result<AccountOperationId, CredentialStoreError> {
	AccountOperationId::new(crate::account_migration::account_migration_gate_uuid(
		run_id,
		slot,
		"operation",
	))
	.map_err(|_| CredentialStoreError::InvalidBundle)
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
fn gate_conflict_credential(
	run: &crate::account_migration::AccountMigrationGateRun,
	variant: u8,
) -> Result<(CredentialBinding, CredentialSecretBundle), CredentialStoreError> {
	if !matches!(variant, 1 | 2) {
		return Err(CredentialStoreError::InvalidBundle);
	}
	let account_id = gate_slot_account_id(&run.run_id, "conflict")?;
	let operation_id = gate_operation_id(&run.run_id, "conflict")?;
	let source = run
		.fixture_root
		.join("protected-store-conflict")
		.join(format!("credential-{variant}.json"));
	crate::account_migration::verify_gate_credential_source(&source)
		.map_err(|_| CredentialStoreError::InvalidBundle)?;
	let imported = crate::account_import::read_explicit_credential_file(
		source.to_str().ok_or(CredentialStoreError::InvalidBundle)?,
	)
	.map_err(|_| CredentialStoreError::InvalidBundle)?;
	if imported.provider.account_id() != format!("xy1422-{}-conflict", run.run_id) {
		return Err(CredentialStoreError::InvalidBundle);
	}
	let target = imported.bundle.binding_for(
		&account_id,
		&operation_id,
		CredentialVersion::new(1).map_err(|_| CredentialStoreError::InvalidBundle)?,
		&imported.provider,
	)?;
	Ok((target, imported.bundle))
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
struct CredentialGatePhase {
	phase: &'static str,
	category: &'static str,
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
#[derive(Debug, Serialize)]
struct CredentialGateConflictReport {
	schema: &'static str,
	complete: bool,
	phases: [CredentialGatePhase; 6],
	readback: Option<AccountMigrationCredentialReadback>,
	primary_failure: Option<CredentialGatePhase>,
	cleanup_failure: Option<CredentialGatePhase>,
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
fn credential_gate_phase(phase: &'static str, category: &'static str) -> CredentialGatePhase {
	CredentialGatePhase { phase, category }
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
fn credential_store_error_category(error: CredentialStoreError) -> &'static str {
	match error {
		CredentialStoreError::Unavailable => "unavailable",
		CredentialStoreError::NotFound => "not_found",
		CredentialStoreError::AlreadyExists => "already_exists",
		CredentialStoreError::VersionConflict => "version_conflict",
		CredentialStoreError::FingerprintMismatch => "fingerprint_mismatch",
		CredentialStoreError::ProviderMismatch => "provider_mismatch",
		CredentialStoreError::AccountMismatch => "account_mismatch",
		CredentialStoreError::WriterMismatch => "writer_mismatch",
		CredentialStoreError::UnsupportedSchema => "unsupported_schema",
		CredentialStoreError::InvalidBundle => "invalid_bundle",
		CredentialStoreError::CorruptBundle => "corrupt_bundle",
	}
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
fn record_credential_gate_failure(
	failure: &mut Option<CredentialGatePhase>,
	phase: &'static str,
	category: &'static str,
) {
	if failure.is_none() {
		*failure = Some(credential_gate_phase(phase, category));
	}
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
fn credential_gate_report_value(
	phases: [CredentialGatePhase; 6],
	readback: Option<AccountMigrationCredentialReadback>,
	primary_failure: Option<CredentialGatePhase>,
	cleanup_failure: Option<CredentialGatePhase>,
) -> Result<Value, CredentialStoreError> {
	let complete = phases.iter().map(|phase| phase.category).eq([
		"created",
		"exact",
		"already_exists",
		"exact_unchanged",
		"deleted",
		"absent",
	]) && primary_failure.is_none()
		&& cleanup_failure.is_none()
		&& readback.is_some();
	serde_json::to_value(CredentialGateConflictReport {
		schema: "decodex/account-migration-credential-gate-conflict/1",
		complete,
		phases,
		readback,
		primary_failure,
		cleanup_failure,
	})
	.map_err(|_| CredentialStoreError::Unavailable)
}

#[cfg(all(test, target_os = "macos", feature = "account-migration-transition-gate"))]
mod credential_gate_report_tests {
	use super::{
		CredentialStoreError, credential_gate_phase, credential_gate_report_value,
		credential_store_error_category,
	};

	#[test]
	fn store_errors_have_closed_categories() {
		let cases = [
			(CredentialStoreError::Unavailable, "unavailable"),
			(CredentialStoreError::NotFound, "not_found"),
			(CredentialStoreError::AlreadyExists, "already_exists"),
			(CredentialStoreError::VersionConflict, "version_conflict"),
			(CredentialStoreError::FingerprintMismatch, "fingerprint_mismatch"),
			(CredentialStoreError::ProviderMismatch, "provider_mismatch"),
			(CredentialStoreError::AccountMismatch, "account_mismatch"),
			(CredentialStoreError::WriterMismatch, "writer_mismatch"),
			(CredentialStoreError::UnsupportedSchema, "unsupported_schema"),
			(CredentialStoreError::InvalidBundle, "invalid_bundle"),
			(CredentialStoreError::CorruptBundle, "corrupt_bundle"),
		];
		for (error, expected) in cases {
			assert_eq!(credential_store_error_category(error), expected);
		}
	}

	#[test]
	fn report_has_fixed_phase_order() {
		let phases = [
			credential_gate_phase("first_add", "created"),
			credential_gate_phase("first_metadata_readback", "exact"),
			credential_gate_phase("duplicate_add", "already_exists"),
			credential_gate_phase("no_overwrite_readback", "exact_unchanged"),
			credential_gate_phase("exact_delete", "deleted"),
			credential_gate_phase("final_absence", "absent"),
		];
		let report = credential_gate_report_value(phases, None, None, None)
			.expect("closed report serializes");
		assert_eq!(report["complete"], false);
		assert_eq!(
			report["phases"]
				.as_array()
				.expect("phases are an array")
				.iter()
				.map(|phase| phase["phase"].as_str().expect("phase is text"))
				.collect::<Vec<_>>(),
			[
				"first_add",
				"first_metadata_readback",
				"duplicate_add",
				"no_overwrite_readback",
				"exact_delete",
				"final_absence",
			]
		);
	}
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
fn prove_gate_create_conflict(
	run: &crate::account_migration::AccountMigrationGateRun,
) -> Result<Value, CredentialStoreError> {
	let account_id = gate_slot_account_id(&run.run_id, "conflict")?;
	let (target, bundle) = gate_conflict_credential(run, 1)?;
	let (conflicting_target, conflicting_bundle) = gate_conflict_credential(run, 2)?;
	let mut phases = [
		credential_gate_phase("first_add", "blocked"),
		credential_gate_phase("first_metadata_readback", "blocked"),
		credential_gate_phase("duplicate_add", "blocked"),
		credential_gate_phase("no_overwrite_readback", "blocked"),
		credential_gate_phase("exact_delete", "not_owned"),
		credential_gate_phase("final_absence", "blocked"),
	];
	let mut primary_failure = None;
	let mut cleanup_failure = None;
	let mut readback = None;
	let store = match MacosKeychainCredentialStore::new(&run.paths) {
		Ok(store) => store,
		Err(error) => {
			let category = credential_store_error_category(error);
			phases[0] = credential_gate_phase("first_add", category);
			record_credential_gate_failure(&mut primary_failure, "first_add", category);
			return credential_gate_report_value(
				phases,
				readback,
				primary_failure,
				cleanup_failure,
			);
		},
	};
	let mut owned_target = None;

	match store.gate_metadata(&account_id) {
		Ok(metadata) if metadata.present => {
			phases[0] = credential_gate_phase("first_add", "not_absent");
			record_credential_gate_failure(&mut primary_failure, "first_add", "not_absent");
		},
		Ok(_) => match store.create(&account_id, &target, bundle) {
			Ok(()) => {
				phases[0] = credential_gate_phase("first_add", "created");
				owned_target = Some(&target);
			},
			Err(error) => {
				let category = credential_store_error_category(error);
				phases[0] = credential_gate_phase("first_add", category);
				record_credential_gate_failure(&mut primary_failure, "first_add", category);
			},
		},
		Err(error) => {
			let category = credential_store_error_category(error);
			phases[0] = credential_gate_phase("first_add", category);
			record_credential_gate_failure(&mut primary_failure, "first_add", category);
		},
	}

	if phases[0].category == "created" {
		match store.read_exact(&account_id, &target).and_then(|bundle| {
			drop(bundle);
			store.gate_metadata(&account_id)
		}) {
			Ok(metadata) if metadata.present => {
				phases[1] = credential_gate_phase("first_metadata_readback", "exact");
				readback = Some(metadata);
			},
			Ok(_) => {
				phases[1] = credential_gate_phase("first_metadata_readback", "mismatch");
				record_credential_gate_failure(
					&mut primary_failure,
					"first_metadata_readback",
					"mismatch",
				);
			},
			Err(error) => {
				let category = credential_store_error_category(error);
				phases[1] = credential_gate_phase("first_metadata_readback", category);
				record_credential_gate_failure(
					&mut primary_failure,
					"first_metadata_readback",
					category,
				);
			},
		}
	}

	let mut duplicate_created = false;
	if primary_failure.is_none() {
		match store.create(&account_id, &conflicting_target, conflicting_bundle) {
			Err(CredentialStoreError::AlreadyExists) => {
				phases[2] = credential_gate_phase("duplicate_add", "already_exists");
			},
			Ok(()) => {
				phases[2] = credential_gate_phase("duplicate_add", "unexpected_success");
				record_credential_gate_failure(
					&mut primary_failure,
					"duplicate_add",
					"unexpected_success",
				);
				duplicate_created = true;
				owned_target = None;
			},
			Err(error) => {
				let category = credential_store_error_category(error);
				phases[2] = credential_gate_phase("duplicate_add", category);
				record_credential_gate_failure(&mut primary_failure, "duplicate_add", category);
			},
		}
	}

	if phases[2].category == "already_exists" {
		match store.read_exact(&account_id, &target).and_then(|bundle| {
			drop(bundle);
			store.gate_metadata(&account_id)
		}) {
			Ok(metadata)
				if metadata.present
					&& readback.as_ref().is_some_and(|before| before == &metadata) =>
			{
				phases[3] = credential_gate_phase("no_overwrite_readback", "exact_unchanged");
			},
			Ok(_) => {
				phases[3] = credential_gate_phase("no_overwrite_readback", "mismatch");
				record_credential_gate_failure(
					&mut primary_failure,
					"no_overwrite_readback",
					"mismatch",
				);
			},
			Err(error) => {
				let category = credential_store_error_category(error);
				phases[3] = credential_gate_phase("no_overwrite_readback", category);
				record_credential_gate_failure(
					&mut primary_failure,
					"no_overwrite_readback",
					category,
				);
			},
		}
	} else if duplicate_created {
		let proved_target = match store.read_exact(&account_id, &target) {
			Ok(bundle) => {
				drop(bundle);
				Some(&target)
			},
			Err(_) => match store.read_exact(&account_id, &conflicting_target) {
				Ok(bundle) => {
					drop(bundle);
					Some(&conflicting_target)
				},
				Err(_) => None,
			},
		};
		if let Some(proved_target) = proved_target {
			owned_target = Some(proved_target);
			match store.gate_metadata(&account_id) {
				Ok(metadata)
					if proved_target == &target
						&& metadata.present
						&& readback.as_ref().is_some_and(|before| before == &metadata) =>
				{
					phases[3] = credential_gate_phase("no_overwrite_readback", "exact_unchanged");
				},
				Ok(_) => {
					phases[3] = credential_gate_phase("no_overwrite_readback", "mismatch");
				},
				Err(error) => {
					phases[3] = credential_gate_phase(
						"no_overwrite_readback",
						credential_store_error_category(error),
					);
				},
			}
		} else {
			phases[3] = credential_gate_phase("no_overwrite_readback", "not_owned");
		}
	}

	if let Some(owned_target) = owned_target {
		match store.delete(&account_id, owned_target) {
			Ok(()) => phases[4] = credential_gate_phase("exact_delete", "deleted"),
			Err(error) => {
				let category = credential_store_error_category(error);
				phases[4] = credential_gate_phase("exact_delete", category);
				record_credential_gate_failure(&mut cleanup_failure, "exact_delete", category);
			},
		}
	}
	match store.gate_metadata(&account_id) {
		Ok(metadata) if metadata.present => {
			phases[5] = credential_gate_phase("final_absence", "present");
			record_credential_gate_failure(&mut cleanup_failure, "final_absence", "present");
		},
		Ok(_) => phases[5] = credential_gate_phase("final_absence", "absent"),
		Err(error) => {
			let category = credential_store_error_category(error);
			phases[5] = credential_gate_phase("final_absence", category);
			record_credential_gate_failure(&mut cleanup_failure, "final_absence", category);
		},
	}

	credential_gate_report_value(phases, readback, primary_failure, cleanup_failure)
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
fn cleanup_gate_run_credentials(
	run: &crate::account_migration::AccountMigrationGateRun,
) -> Result<Value, CredentialStoreError> {
	let expected_account_ids = ["account_1", "account_2", "account_3", "account_4"]
		.into_iter()
		.map(|slot| gate_slot_account_id(&run.run_id, slot))
		.collect::<Result<Vec<_>, _>>()?;
	let manifest_path = run.paths.root().as_path().join("account-migration-manifest.json");
	let store = MacosKeychainCredentialStore::new(&run.paths)?;
	let mut deleted = 0_u32;
	let conflict_account_id = gate_slot_account_id(&run.run_id, "conflict")?;
	let (conflict_binding_1, conflict_bundle_1) = gate_conflict_credential(run, 1)?;
	drop(conflict_bundle_1);
	let (conflict_binding_2, conflict_bundle_2) = gate_conflict_credential(run, 2)?;
	drop(conflict_bundle_2);
	if store.gate_metadata(&conflict_account_id)?.present {
		let owned_binding = match store.read_exact(&conflict_account_id, &conflict_binding_1) {
			Ok(bundle) => {
				drop(bundle);
				&conflict_binding_1
			},
			Err(_) => match store.read_exact(&conflict_account_id, &conflict_binding_2) {
				Ok(bundle) => {
					drop(bundle);
					&conflict_binding_2
				},
				Err(error) => return Err(error),
			},
		};
		store.delete(&conflict_account_id, owned_binding)?;
		if store.gate_metadata(&conflict_account_id)?.present {
			return Err(CredentialStoreError::Unavailable);
		}
		deleted += 1;
	}
	let bindings = match std::fs::symlink_metadata(&manifest_path) {
		Ok(_) => crate::account_migration::account_migration_gate_manifest_bindings(
			&manifest_path,
			&expected_account_ids,
		)
		.map_err(|_| CredentialStoreError::InvalidBundle)?,
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
			for account_id in &expected_account_ids {
				if store.gate_metadata(account_id)?.present {
					return Err(CredentialStoreError::InvalidBundle);
				}
			}
			Vec::new()
		},
		Err(_) => return Err(CredentialStoreError::InvalidBundle),
	};
	for (account_id, binding) in bindings.iter().rev() {
		let metadata = store.gate_metadata(account_id)?;
		if !metadata.present {
			continue;
		}
		drop(store.read_exact(account_id, binding)?);
		store.delete(account_id, binding)?;
		if store.gate_metadata(account_id)?.present {
			return Err(CredentialStoreError::Unavailable);
		}
		deleted += 1;
	}
	for account_id in expected_account_ids.iter().chain([&conflict_account_id]) {
		if store.gate_metadata(account_id)?.present {
			return Err(CredentialStoreError::Unavailable);
		}
	}
	Ok(json!({
		"schema": "decodex/account-migration-credential-gate-cleanup/1",
		"finite_slot_count": 5,
		"manifest_account_count": bindings.len(),
		"conflict_slot_checked": true,
		"deleted": deleted,
		"absence_verified": true
	}))
}
