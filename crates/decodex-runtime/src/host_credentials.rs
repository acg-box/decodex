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
const MAX_CREDENTIAL_RECORD_BYTES: usize = 1024 * 1024;

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
	/// Another account record already owns the same provider identity.
	DuplicateProvider,
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
			Self::DuplicateProvider => "host credential provider already exists",
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
	let bytes = serde_json::to_vec(persisted).map_err(|_| CredentialStoreError::InvalidBundle)?;
	if bytes.len() > MAX_CREDENTIAL_RECORD_BYTES {
		return Err(CredentialStoreError::InvalidBundle);
	}

	Ok(Zeroizing::new(bytes))
}

fn decode(
	bytes: Vec<u8>,
) -> Result<(PersistedCredentialV1, CredentialFingerprint), CredentialStoreError> {
	if bytes.len() > MAX_CREDENTIAL_RECORD_BYTES {
		return Err(CredentialStoreError::CorruptBundle);
	}
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

#[cfg(unix)]
mod redb_store {
	use std::fmt::{Debug, Formatter};

	use decodex_core::DecodexPaths;
	use redb::{Database, Durability, ReadableDatabase as _, ReadableTable as _, TableDefinition};

	use super::{
		AccountId, CredentialBinding, CredentialSecretBundle, CredentialStoreError,
		CredentialVersion, HostCredentialStore, PersistedCredentialV1, StoredCredential, decode,
		encode, enforce_exact, fingerprint, seal_exact_read,
	};

	const CREDENTIALS: TableDefinition<&str, &[u8]> =
		TableDefinition::new("account_credentials_v1");

	/// Daemon-owned ACID credential vault backed by one private `redb` file.
	pub struct RedbCredentialStore {
		database: Database,
	}
	impl RedbCredentialStore {
		/// Open or create the canonical private vault and initialize its closed table.
		pub fn new(paths: &DecodexPaths) -> Result<Self, CredentialStoreError> {
			let file = paths
				.open_credential_vault_file()
				.map_err(|_| CredentialStoreError::Unavailable)?;
			let database = Database::builder()
				.create_file(file)
				.map_err(|_| CredentialStoreError::Unavailable)?;
			let mut transaction = database
				.begin_write()
				.map_err(|_| CredentialStoreError::Unavailable)?;
			transaction
				.set_durability(Durability::Immediate)
				.map_err(|_| CredentialStoreError::Unavailable)?;
			{
				transaction
					.open_table(CREDENTIALS)
					.map_err(|_| CredentialStoreError::Unavailable)?;
			}
			transaction.commit().map_err(|_| CredentialStoreError::Unavailable)?;

			Ok(Self { database })
		}

		fn read_record(
			&self,
			account_id: &AccountId,
		) -> Result<(PersistedCredentialV1, CredentialBinding), CredentialStoreError> {
			let transaction =
				self.database.begin_read().map_err(|_| CredentialStoreError::Unavailable)?;
			let table =
				transaction.open_table(CREDENTIALS).map_err(|_| CredentialStoreError::Unavailable)?;
			let value = table
				.get(account_id.as_str())
				.map_err(|_| CredentialStoreError::Unavailable)?
				.ok_or(CredentialStoreError::NotFound)?;
			let (persisted, fingerprint) = decode(value.value().to_vec())?;
			if persisted.account_id()? != *account_id {
				return Err(CredentialStoreError::AccountMismatch);
			}
			let binding = persisted.binding(fingerprint)?;

			Ok((persisted, binding))
		}

		fn ensure_provider_available(
			table: &redb::Table<'_, &str, &[u8]>,
			account_id: &AccountId,
			target: &CredentialBinding,
		) -> Result<(), CredentialStoreError> {
			let records = table.iter().map_err(|_| CredentialStoreError::Unavailable)?;
			for record in records {
				let (stored_account, stored_bytes) =
					record.map_err(|_| CredentialStoreError::Unavailable)?;
				if stored_account.value() == account_id.as_str() {
					continue;
				}
				let (persisted, stored_fingerprint) = decode(stored_bytes.value().to_vec())?;
				let stored_binding = persisted.binding(stored_fingerprint)?;
				if stored_binding.provider == target.provider {
					return Err(CredentialStoreError::DuplicateProvider);
				}
			}

			Ok(())
		}

		fn begin_immediate_write(&self) -> Result<redb::WriteTransaction, CredentialStoreError> {
			let mut transaction =
				self.database.begin_write().map_err(|_| CredentialStoreError::Unavailable)?;
			transaction
				.set_durability(Durability::Immediate)
				.map_err(|_| CredentialStoreError::Unavailable)?;

			Ok(transaction)
		}
	}
	impl Debug for RedbCredentialStore {
		fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
			formatter.write_str("RedbCredentialStore([REDACTED])")
		}
	}
	impl HostCredentialStore for RedbCredentialStore {
		fn create(
			&self,
			account_id: &AccountId,
			target: &CredentialBinding,
			bundle: CredentialSecretBundle,
		) -> Result<(), CredentialStoreError> {
			if target.version.get() != 1 {
				return Err(CredentialStoreError::VersionConflict);
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

			let transaction = self.begin_immediate_write()?;
			{
				let mut table = transaction
					.open_table(CREDENTIALS)
					.map_err(|_| CredentialStoreError::Unavailable)?;
				if table
					.get(account_id.as_str())
					.map_err(|_| CredentialStoreError::Unavailable)?
					.is_some()
				{
					return Err(CredentialStoreError::AlreadyExists);
				}
				Self::ensure_provider_available(&table, account_id, target)?;
				table
					.insert(account_id.as_str(), bytes.as_slice())
					.map_err(|_| CredentialStoreError::Unavailable)?;
			}
			transaction.commit().map_err(|_| CredentialStoreError::Unavailable)
		}

		fn read_exact(
			&self,
			account_id: &AccountId,
			expected: &CredentialBinding,
		) -> Result<StoredCredential, CredentialStoreError> {
			let (persisted, binding) = self.read_record(account_id)?;
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
			let target_binding = persisted.binding(fingerprint(&bytes)?)?;
			enforce_exact(&target_binding, target)?;

			let transaction = self.begin_immediate_write()?;
			{
				let mut table = transaction
					.open_table(CREDENTIALS)
					.map_err(|_| CredentialStoreError::Unavailable)?;
				let actual_bytes = table
					.get(account_id.as_str())
					.map_err(|_| CredentialStoreError::Unavailable)?
					.map(|value| value.value().to_vec())
					.ok_or(CredentialStoreError::NotFound)?;
				let (actual, actual_fingerprint) = decode(actual_bytes)?;
				if actual.account_id()? != *account_id {
					return Err(CredentialStoreError::AccountMismatch);
				}
				enforce_exact(&actual.binding(actual_fingerprint)?, expected)?;
				Self::ensure_provider_available(&table, account_id, target)?;
				table
					.insert(account_id.as_str(), bytes.as_slice())
					.map_err(|_| CredentialStoreError::Unavailable)?;
			}
			transaction.commit().map_err(|_| CredentialStoreError::Unavailable)
		}

		fn delete(
			&self,
			account_id: &AccountId,
			expected: &CredentialBinding,
		) -> Result<(), CredentialStoreError> {
			let transaction = self.begin_immediate_write()?;
			{
				let mut table = transaction
					.open_table(CREDENTIALS)
					.map_err(|_| CredentialStoreError::Unavailable)?;
				let actual_bytes = table
					.get(account_id.as_str())
					.map_err(|_| CredentialStoreError::Unavailable)?
					.map(|value| value.value().to_vec())
					.ok_or(CredentialStoreError::NotFound)?;
				let (actual, actual_fingerprint) = decode(actual_bytes)?;
				if actual.account_id()? != *account_id {
					return Err(CredentialStoreError::AccountMismatch);
				}
				enforce_exact(&actual.binding(actual_fingerprint)?, expected)?;
				table
					.remove(account_id.as_str())
					.map_err(|_| CredentialStoreError::Unavailable)?;
			}
			transaction.commit().map_err(|_| CredentialStoreError::Unavailable)
		}
	}

	#[cfg(test)]
	mod tests {
		use decodex_core::{
			AccountId, AccountOperationId, AccountProvider, CredentialBinding, CredentialVersion,
			DecodexRoot, ProviderIdentity,
		};

		use super::{CredentialSecretBundle, CredentialStoreError, HostCredentialStore};
		use super::RedbCredentialStore;

		fn account(value: u16) -> AccountId {
			AccountId::new(format!("00000000-0000-4000-8000-{value:012}"))
				.expect("test account id")
		}

		fn operation(value: u16) -> AccountOperationId {
			AccountOperationId::new(format!("10000000-0000-4000-8000-{value:012}"))
				.expect("test operation id")
		}

		fn bundle(provider: &ProviderIdentity, material: &str) -> CredentialSecretBundle {
			CredentialSecretBundle::chatgpt(
				format!("access-{material}"),
				format!("refresh-{material}"),
				Some(format!("identity-{material}")),
				Some("pro".to_owned()),
				provider.account_id().to_owned(),
				"bearer".to_owned(),
				4_102_444_800_000_000,
			)
			.expect("valid test bundle")
		}

		fn credential(
			account_id: &AccountId,
			provider_account: &str,
			version: u64,
			operation_value: u16,
			material: &str,
		) -> (CredentialSecretBundle, CredentialBinding) {
			let provider =
				ProviderIdentity::new(AccountProvider::Chatgpt, provider_account).expect("provider");
			let bundle = bundle(&provider, material);
			let binding = bundle
				.binding_for(
					account_id,
					&operation(operation_value),
					CredentialVersion::new(version).expect("version"),
					&provider,
				)
				.expect("binding");
			(bundle, binding)
		}

		#[test]
		fn create_read_restart_rotate_and_delete_preserve_exact_contract() {
			let temporary = tempfile::tempdir().expect("temporary root");
			let temporary_root = temporary.path().canonicalize().expect("canonical temporary root");
			let root = DecodexRoot::new(temporary_root.join("decodex-root")).expect("safe root");
			let paths = root.paths();
			let account_id = account(1);
			let (initial_bundle, initial) =
				credential(&account_id, "provider-1", 1, 1, "initial");

			let store = RedbCredentialStore::new(&paths).expect("open vault");
			store.create(&account_id, &initial, initial_bundle).expect("create");
			let read = store.read_exact(&account_id, &initial).expect("read initial");
			assert_eq!(read.bundle().access_token(), "access-initial");
			drop(store);

			let store = RedbCredentialStore::new(&paths).expect("reopen vault");
			assert_eq!(
				store.read_exact(&account_id, &initial).expect("restart read").binding(),
				&initial,
			);
			let (rotated_bundle, rotated) =
				credential(&account_id, "provider-1", 2, 2, "rotated");
			store
				.compare_and_swap_rotate(&account_id, &initial, &rotated, rotated_bundle)
				.expect("rotate");
			assert!(matches!(
				store.read_exact(&account_id, &initial),
				Err(CredentialStoreError::VersionConflict),
			));
			assert_eq!(
				store
					.read_exact(&account_id, &rotated)
					.expect("read rotated")
					.bundle()
					.access_token(),
				"access-rotated",
			);
			store.delete(&account_id, &rotated).expect("delete exact");
			assert!(matches!(
				store.read_exact(&account_id, &rotated),
				Err(CredentialStoreError::NotFound),
			));
		}

		#[test]
		fn duplicate_provider_and_failed_cas_leave_existing_records_unchanged() {
			let temporary = tempfile::tempdir().expect("temporary root");
			let temporary_root = temporary.path().canonicalize().expect("canonical temporary root");
			let root = DecodexRoot::new(temporary_root.join("decodex-root")).expect("safe root");
			let paths = root.paths();
			let first_account = account(1);
			let second_account = account(2);
			let (first_bundle, first) =
				credential(&first_account, "shared-provider", 1, 1, "first");
			let (second_bundle, second) =
				credential(&second_account, "shared-provider", 1, 2, "second");
			let store = RedbCredentialStore::new(&paths).expect("open vault");
			store.create(&first_account, &first, first_bundle).expect("create first");
			assert_eq!(
				store.create(&second_account, &second, second_bundle),
				Err(CredentialStoreError::DuplicateProvider),
			);
			assert!(matches!(
				store.read_exact(&second_account, &second),
				Err(CredentialStoreError::NotFound),
			));

			let (bad_bundle, mut bad_target) =
				credential(&first_account, "shared-provider", 2, 3, "bad-cas");
			bad_target.fingerprint = first.fingerprint.clone();
			assert_eq!(
				store.compare_and_swap_rotate(&first_account, &first, &bad_target, bad_bundle),
				Err(CredentialStoreError::FingerprintMismatch),
			);
			assert_eq!(
				store.read_exact(&first_account, &first).expect("original remains").binding(),
				&first,
			);
		}

		#[test]
		fn one_process_owns_the_writable_vault() {
			let temporary = tempfile::tempdir().expect("temporary root");
			let temporary_root = temporary.path().canonicalize().expect("canonical temporary root");
			let root = DecodexRoot::new(temporary_root.join("decodex-root")).expect("safe root");
			let paths = root.paths();
			let _first = RedbCredentialStore::new(&paths).expect("first writer");

			assert!(matches!(
				RedbCredentialStore::new(&paths),
				Err(CredentialStoreError::Unavailable),
			));
		}
	}
}

#[cfg(unix)] pub use redb_store::RedbCredentialStore;
