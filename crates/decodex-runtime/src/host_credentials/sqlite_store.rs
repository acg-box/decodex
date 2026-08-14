//! Credential adapter over the daemon-owned product database.

use std::fmt::{Debug, Formatter};

use decodex_core::{AccountId, CredentialBinding, CredentialVersion};
use decodex_database::{CredentialKey, CredentialRecord, DatabaseError, SqliteStore};

use super::{
	CredentialSecretBundle, CredentialStoreError, HostCredentialStore, PersistedCredentialV1,
	StoredCredential, decode, encode, enforce_exact, fingerprint, provider_text, seal_exact_read,
};

/// Narrow secret-bearing adapter over the same physical SQLite product authority.
pub struct SqliteCredentialStore {
	store: SqliteStore,
}

impl SqliteCredentialStore {
	/// Bind credential access to the already-open daemon store.
	pub fn new(store: SqliteStore) -> Self {
		Self { store }
	}

	fn read_record(
		&self,
		account_id: &AccountId,
	) -> Result<(PersistedCredentialV1, CredentialBinding), CredentialStoreError> {
		let record = self.store.read_credential(account_id.as_str()).map_err(map_read_error)?;
		let (persisted, fingerprint) = decode(record.payload.to_vec())?;
		if persisted.account_id()? != *account_id {
			return Err(CredentialStoreError::AccountMismatch);
		}
		let binding = persisted.binding(fingerprint)?;
		if credential_key(&binding, account_id) != record.key {
			return Err(CredentialStoreError::CorruptBundle);
		}
		Ok((persisted, binding))
	}
}

impl Debug for SqliteCredentialStore {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("SqliteCredentialStore([REDACTED])")
	}
}

impl HostCredentialStore for SqliteCredentialStore {
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
		self.store
			.create_credential(CredentialRecord {
				key: credential_key(target, account_id),
				payload: bytes,
			})
			.map_err(map_create_error)
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
		let (_, actual) = self.read_record(account_id)?;
		enforce_exact(&actual, expected)?;
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
		self.store
			.rotate_credential(
				&credential_key(expected, account_id),
				CredentialRecord { key: credential_key(target, account_id), payload: bytes },
			)
			.map_err(map_compare_and_swap_error)
	}

	fn delete(
		&self,
		account_id: &AccountId,
		expected: &CredentialBinding,
	) -> Result<(), CredentialStoreError> {
		let (_, actual) = self.read_record(account_id)?;
		enforce_exact(&actual, expected)?;
		self.store
			.delete_credential(&credential_key(expected, account_id))
			.map_err(map_delete_error)
	}
}

fn credential_key(binding: &CredentialBinding, account_id: &AccountId) -> CredentialKey {
	CredentialKey {
		account_id: account_id.as_str().to_owned(),
		schema_version: binding.schema_version.get(),
		credential_version: binding.version.get(),
		fingerprint: binding.fingerprint.as_str().to_owned(),
		writer_operation_id: binding.writer_operation_id.as_str().to_owned(),
		provider: provider_text(binding.provider.provider()).to_owned(),
		provider_account_id: binding.provider.account_id().to_owned(),
	}
}

const fn map_read_error(error: DatabaseError) -> CredentialStoreError {
	match error {
		DatabaseError::NotFound => CredentialStoreError::NotFound,
		DatabaseError::Corrupt | DatabaseError::Incompatible => CredentialStoreError::CorruptBundle,
		_ => CredentialStoreError::Unavailable,
	}
}

const fn map_create_error(error: DatabaseError) -> CredentialStoreError {
	match error {
		DatabaseError::AlreadyExists => CredentialStoreError::AlreadyExists,
		DatabaseError::Conflict => CredentialStoreError::DuplicateProvider,
		_ => CredentialStoreError::Unavailable,
	}
}

const fn map_compare_and_swap_error(error: DatabaseError) -> CredentialStoreError {
	match error {
		DatabaseError::NotFound => CredentialStoreError::NotFound,
		DatabaseError::Conflict => CredentialStoreError::VersionConflict,
		_ => CredentialStoreError::Unavailable,
	}
}

const fn map_delete_error(error: DatabaseError) -> CredentialStoreError {
	match error {
		DatabaseError::NotFound => CredentialStoreError::NotFound,
		DatabaseError::Conflict => CredentialStoreError::VersionConflict,
		_ => CredentialStoreError::Unavailable,
	}
}
