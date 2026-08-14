use std::fmt::{Debug, Formatter};

use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::{DatabaseError, SqliteStore, error::sqlite_error, unix_micros};

/// Credential-negative key used for exact compare-and-swap operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialKey {
	pub account_id: String,
	pub schema_version: u16,
	pub credential_version: u64,
	pub fingerprint: String,
	pub writer_operation_id: String,
	pub provider: String,
	pub provider_account_id: String,
}

/// Secret-bearing database record. Debug output never includes its payload.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct CredentialRecord {
	#[zeroize(skip)]
	pub key: CredentialKey,
	pub payload: Zeroizing<Vec<u8>>,
}

impl Debug for CredentialRecord {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("CredentialRecord")
			.field("key", &self.key)
			.field("payload", &"[REDACTED]")
			.finish()
	}
}

impl SqliteStore {
	pub fn create_credential(&self, record: CredentialRecord) -> Result<(), DatabaseError> {
		self.with_connection(|connection| {
			let now = unix_micros()?;
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sqlite_error)?;
			transaction
				.execute(
					"INSERT OR IGNORE INTO account_identities (account_id, created_at_micros)
					 VALUES (?1, ?2)",
					params![record.key.account_id, now],
				)
				.map_err(sqlite_error)?;
			let inserted = transaction
				.execute(
					"INSERT INTO account_credentials (
					   account_id, schema_version, credential_version, fingerprint,
					   writer_operation_id, provider, provider_account_id, payload, updated_at_micros
					 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
					 ON CONFLICT(account_id) DO NOTHING",
					params![
						record.key.account_id,
						i64::from(record.key.schema_version),
						i64::try_from(record.key.credential_version)
							.map_err(|_| DatabaseError::Conflict)?,
						record.key.fingerprint,
						record.key.writer_operation_id,
						record.key.provider,
						record.key.provider_account_id,
						record.payload.as_slice(),
						now,
					],
				)
				.map_err(|error| match error {
					rusqlite::Error::SqliteFailure(inner, _)
						if inner.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
						DatabaseError::Conflict,
					_ => sqlite_error(error),
				})?;
			if inserted != 1 {
				return Err(DatabaseError::AlreadyExists);
			}
			transaction.commit().map_err(sqlite_error)
		})
	}

	pub fn read_credential(&self, account_id: &str) -> Result<CredentialRecord, DatabaseError> {
		self.with_connection(|connection| {
			connection
				.query_row(
					"SELECT schema_version, credential_version, fingerprint, writer_operation_id,
					        provider, provider_account_id, payload
					 FROM account_credentials WHERE account_id = ?1",
					params![account_id],
					|row| {
						let version = row.get::<_, i64>(1)?;
						let schema = row.get::<_, i64>(0)?;
						Ok((
							schema,
							version,
							row.get::<_, String>(2)?,
							row.get::<_, String>(3)?,
							row.get::<_, String>(4)?,
							row.get::<_, String>(5)?,
							row.get::<_, Vec<u8>>(6)?,
						))
					},
				)
				.optional()
				.map_err(sqlite_error)?
				.map(
					|(
						schema,
						version,
						fingerprint,
						writer,
						provider,
						provider_account,
						payload,
					)| {
						Ok(CredentialRecord {
							key: CredentialKey {
								account_id: account_id.to_owned(),
								schema_version: u16::try_from(schema)
									.map_err(|_| DatabaseError::Corrupt)?,
								credential_version: u64::try_from(version)
									.map_err(|_| DatabaseError::Corrupt)?,
								fingerprint,
								writer_operation_id: writer,
								provider,
								provider_account_id: provider_account,
							},
							payload: Zeroizing::new(payload),
						})
					},
				)
				.transpose()?
				.ok_or(DatabaseError::NotFound)
		})
	}

	pub fn rotate_credential(
		&self,
		expected: &CredentialKey,
		target: CredentialRecord,
	) -> Result<(), DatabaseError> {
		if expected.account_id != target.key.account_id
			|| expected.provider != target.key.provider
			|| expected.provider_account_id != target.key.provider_account_id
			|| target.key.credential_version != expected.credential_version.saturating_add(1)
		{
			return Err(DatabaseError::Conflict);
		}
		self.with_connection(|connection| {
			let changed = connection
				.execute(
					"UPDATE account_credentials
					 SET schema_version = ?1, credential_version = ?2, fingerprint = ?3,
					     writer_operation_id = ?4, provider = ?5, provider_account_id = ?6,
					     payload = ?7, updated_at_micros = ?8
					 WHERE account_id = ?9 AND schema_version = ?10 AND credential_version = ?11
					   AND fingerprint = ?12 AND writer_operation_id = ?13
					   AND provider = ?14 AND provider_account_id = ?15",
					params![
						i64::from(target.key.schema_version),
						i64::try_from(target.key.credential_version)
							.map_err(|_| DatabaseError::Conflict)?,
						target.key.fingerprint,
						target.key.writer_operation_id,
						target.key.provider,
						target.key.provider_account_id,
						target.payload.as_slice(),
						unix_micros()?,
						expected.account_id,
						i64::from(expected.schema_version),
						i64::try_from(expected.credential_version)
							.map_err(|_| DatabaseError::Conflict)?,
						expected.fingerprint,
						expected.writer_operation_id,
						expected.provider,
						expected.provider_account_id,
					],
				)
				.map_err(sqlite_error)?;
			if changed == 1 { Ok(()) } else { Err(DatabaseError::Conflict) }
		})
	}

	pub fn delete_credential(&self, expected: &CredentialKey) -> Result<(), DatabaseError> {
		self.with_connection(|connection| {
			let changed = connection
				.execute(
					"DELETE FROM account_credentials
					 WHERE account_id = ?1 AND schema_version = ?2 AND credential_version = ?3
					   AND fingerprint = ?4 AND writer_operation_id = ?5
					   AND provider = ?6 AND provider_account_id = ?7",
					params![
						expected.account_id,
						i64::from(expected.schema_version),
						i64::try_from(expected.credential_version)
							.map_err(|_| DatabaseError::Conflict)?,
						expected.fingerprint,
						expected.writer_operation_id,
						expected.provider,
						expected.provider_account_id,
					],
				)
				.map_err(sqlite_error)?;
			if changed == 1 { Ok(()) } else { Err(DatabaseError::NotFound) }
		})
	}
}
