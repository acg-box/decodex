//! One-shot transfer into the local SQLite account authority.

use std::{
	collections::BTreeSet,
	fmt::{Debug, Display, Formatter},
};

use decodex_core::{
	AccountProvider, AccountQuotaDisposition, AccountQuotaObservationError,
	AccountQuotaWindowObservation, AccountRecord, AccountRoutingControl, AccountSelectionMode,
	AccountState, CredentialBinding,
};
use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};
use serde_json::json;

use crate::{CredentialRecord, DatabaseError, SqliteStore, error::sqlite_error, unix_micros};

const MAX_ACCOUNTS: usize = 512;

/// One account and its exact secret-bearing credential record.
pub struct LocalAccountTransfer {
	pub account: AccountRecord,
	pub credential: CredentialRecord,
}

impl Debug for LocalAccountTransfer {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("LocalAccountTransfer")
			.field("account", &self.account)
			.field("credential", &self.credential)
			.finish()
	}
}

/// Complete immutable source snapshot accepted by the one-shot transfer.
pub struct LocalAccountTransferBatch {
	pub source_sha256: String,
	pub accounts: Vec<LocalAccountTransfer>,
	pub routing: AccountRoutingControl,
}

impl Debug for LocalAccountTransferBatch {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("LocalAccountTransferBatch")
			.field("source_sha256", &self.source_sha256)
			.field("account_count", &self.accounts.len())
			.field("routing", &self.routing)
			.finish()
	}
}

/// Stable outcome of an exact one-shot transfer attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalAccountTransferOutcome {
	Imported { account_count: u16 },
	Replayed { account_count: u16 },
}

/// Secret-negative transfer refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalAccountTransferError {
	InvalidInput,
	TargetNotFresh,
	Database(DatabaseError),
}

impl Display for LocalAccountTransferError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidInput => "local account transfer input is invalid",
			Self::TargetNotFresh => "local account transfer target is not fresh",
			Self::Database(_) => "local account transfer database is unavailable",
		})
	}
}

impl std::error::Error for LocalAccountTransferError {}

impl From<DatabaseError> for LocalAccountTransferError {
	fn from(value: DatabaseError) -> Self {
		Self::Database(value)
	}
}

impl SqliteStore {
	/// Import one exact account snapshot atomically, or replay the same completed transfer.
	pub fn import_local_accounts(
		&self,
		batch: LocalAccountTransferBatch,
	) -> Result<LocalAccountTransferOutcome, LocalAccountTransferError> {
		validate_batch(&batch)?;
		self.with_connection(|connection| {
			import_sync(connection, batch).map_err(map_transfer_error)
		})
		.map_err(|error| match error {
			DatabaseError::Conflict => LocalAccountTransferError::InvalidInput,
			DatabaseError::AlreadyExists => LocalAccountTransferError::TargetNotFresh,
			other => LocalAccountTransferError::Database(other),
		})
	}
}

fn map_transfer_error(error: LocalAccountTransferError) -> DatabaseError {
	match error {
		LocalAccountTransferError::InvalidInput => DatabaseError::Conflict,
		LocalAccountTransferError::TargetNotFresh => DatabaseError::AlreadyExists,
		LocalAccountTransferError::Database(error) => error,
	}
}

fn import_sync(
	connection: &mut Connection,
	batch: LocalAccountTransferBatch,
) -> Result<LocalAccountTransferOutcome, LocalAccountTransferError> {
	let transaction = connection
		.transaction_with_behavior(TransactionBehavior::Immediate)
		.map_err(|error| LocalAccountTransferError::Database(sqlite_error(error)))?;
	let prior = transaction
		.query_row(
			"SELECT source_sha256, account_count FROM local_account_transfers WHERE singleton = 1",
			[],
			|row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
		)
		.optional()
		.map_err(|error| LocalAccountTransferError::Database(sqlite_error(error)))?;
	if let Some((source_sha256, account_count)) = prior {
		let expected_count = i64::try_from(batch.accounts.len())
			.map_err(|_| LocalAccountTransferError::InvalidInput)?;
		if source_sha256 != batch.source_sha256 || account_count != expected_count {
			return Err(LocalAccountTransferError::TargetNotFresh);
		}
		let actual_count: i64 = transaction
			.query_row("SELECT COUNT(*) FROM accounts", [], |row| row.get(0))
			.map_err(|error| LocalAccountTransferError::Database(sqlite_error(error)))?;
		if actual_count != expected_count {
			return Err(LocalAccountTransferError::TargetNotFresh);
		}
		transaction
			.commit()
			.map_err(|error| LocalAccountTransferError::Database(sqlite_error(error)))?;
		return Ok(LocalAccountTransferOutcome::Replayed {
			account_count: u16::try_from(expected_count)
				.map_err(|_| LocalAccountTransferError::InvalidInput)?,
		});
	}
	if mutable_target_rows(&transaction)? != 0 {
		return Err(LocalAccountTransferError::TargetNotFresh);
	}

	let now = unix_micros().map_err(LocalAccountTransferError::Database)?;
	for transferred in &batch.accounts {
		insert_account(&transaction, transferred, now)?;
	}
	for (position, account_id) in batch.routing.order.iter().enumerate() {
		transaction
			.execute(
				"INSERT INTO account_routing_order (account_id, position, updated_at_micros)
				 VALUES (?1, ?2, ?3)",
				params![
					account_id.as_str(),
					i64::try_from(position).map_err(|_| LocalAccountTransferError::InvalidInput)?,
					now,
				],
			)
			.map_err(|error| LocalAccountTransferError::Database(sqlite_error(error)))?;
	}
	let (mode, fixed_account_id) = match &batch.routing.mode {
		AccountSelectionMode::Balanced => ("balanced", None),
		AccountSelectionMode::Fixed(account_id) => ("fixed", Some(account_id.as_str())),
	};
	transaction
		.execute(
			"UPDATE account_routing_control
			 SET mode = ?1, fixed_account_id = ?2, revision = ?3, updated_at_micros = ?4
			 WHERE singleton = 1",
			params![mode, fixed_account_id, batch.routing.revision, now],
		)
		.map_err(|error| LocalAccountTransferError::Database(sqlite_error(error)))?;
	transaction
		.execute(
			"INSERT INTO local_account_transfers
			 (singleton, source_sha256, account_count, imported_at_micros)
			 VALUES (1, ?1, ?2, ?3)",
			params![
				batch.source_sha256,
				i64::try_from(batch.accounts.len())
					.map_err(|_| LocalAccountTransferError::InvalidInput)?,
				now,
			],
		)
		.map_err(|error| LocalAccountTransferError::Database(sqlite_error(error)))?;
	transaction
		.commit()
		.map_err(|error| LocalAccountTransferError::Database(sqlite_error(error)))?;

	Ok(LocalAccountTransferOutcome::Imported {
		account_count: u16::try_from(batch.accounts.len())
			.map_err(|_| LocalAccountTransferError::InvalidInput)?,
	})
}

fn mutable_target_rows(connection: &Connection) -> Result<i64, LocalAccountTransferError> {
	connection
		.query_row(
			"SELECT
			   (SELECT COUNT(*) FROM accounts) +
			   (SELECT COUNT(*) FROM account_credentials) +
			   (SELECT COUNT(*) FROM account_operations) +
			   (SELECT COUNT(*) FROM conversations) +
			   (SELECT COUNT(*) FROM runtime_sessions) +
			   (SELECT COUNT(*) FROM command_receipts)",
			[],
			|row| row.get(0),
		)
		.map_err(|error| LocalAccountTransferError::Database(sqlite_error(error)))
}

fn insert_account(
	connection: &Connection,
	transferred: &LocalAccountTransfer,
	now: i64,
) -> Result<(), LocalAccountTransferError> {
	let account = &transferred.account;
	let binding = account.credential.as_ref().ok_or(LocalAccountTransferError::InvalidInput)?;
	let target_json = serde_json::to_string(&json!({
		"schema_version": binding.schema_version.get(),
		"credential_version": binding.version.get(),
		"fingerprint": binding.fingerprint.as_str(),
		"writer_operation_id": binding.writer_operation_id.as_str(),
		"provider": provider_text(binding.provider.provider()),
		"provider_account_id": binding.provider.account_id(),
	}))
	.map_err(|_| LocalAccountTransferError::InvalidInput)?;

	connection
		.execute(
			"INSERT INTO account_identities (account_id, created_at_micros) VALUES (?1, ?2)",
			params![account.account_id.as_str(), now],
		)
		.map_err(|error| LocalAccountTransferError::Database(sqlite_error(error)))?;
	connection
		.execute(
			"INSERT INTO account_operations (
			   operation_id, account_id, kind, phase, expected_account_revision,
			   expected_credential_json, target_credential_json, provider,
			   provider_account_id, requested_display_label, requested_enabled,
			   recovery_code, created_at_micros, updated_at_micros, completed_at_micros
			 ) VALUES (?1, ?2, 'import', 'committed', NULL, NULL, ?3, ?4, ?5, ?6, ?7,
			           NULL, ?8, ?8, ?8)",
			params![
				binding.writer_operation_id.as_str(),
				account.account_id.as_str(),
				target_json,
				provider_text(binding.provider.provider()),
				binding.provider.account_id(),
				account.label,
				account.enabled,
				now,
			],
		)
		.map_err(|error| LocalAccountTransferError::Database(sqlite_error(error)))?;
	connection
		.execute(
			"INSERT INTO accounts (
			   account_id, display_label, enabled, state, revision, provider,
			   provider_account_id, credential_store_observation,
			   created_at_micros, updated_at_micros, tombstoned_at_micros
			 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'exact', ?8, ?8, NULL)",
			params![
				account.account_id.as_str(),
				account.label,
				account.enabled,
				state_text(account.observed_state),
				account.revision,
				provider_text(binding.provider.provider()),
				binding.provider.account_id(),
				now,
			],
		)
		.map_err(|error| LocalAccountTransferError::Database(sqlite_error(error)))?;
	connection
		.execute(
			"INSERT INTO account_credentials (
			   account_id, schema_version, credential_version, fingerprint,
			   writer_operation_id, provider, provider_account_id, payload, updated_at_micros
			 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				transferred.credential.key.account_id,
				i64::from(transferred.credential.key.schema_version),
				i64::try_from(transferred.credential.key.credential_version)
					.map_err(|_| LocalAccountTransferError::InvalidInput)?,
				transferred.credential.key.fingerprint,
				transferred.credential.key.writer_operation_id,
				transferred.credential.key.provider,
				transferred.credential.key.provider_account_id,
				transferred.credential.payload.as_slice(),
				now,
			],
		)
		.map_err(|error| LocalAccountTransferError::Database(sqlite_error(error)))?;
	insert_quota(connection, account.account_id.as_str(), account.five_hour_quota)?;
	insert_quota(connection, account.account_id.as_str(), account.seven_day_quota)
}

fn insert_quota(
	connection: &Connection,
	account_id: &str,
	observation: AccountQuotaWindowObservation,
) -> Result<(), LocalAccountTransferError> {
	let Some(observed_at) = observation.observed_at_unix_micros else {
		return if matches!(observation.disposition, AccountQuotaDisposition::Unknown) {
			Ok(())
		} else {
			Err(LocalAccountTransferError::InvalidInput)
		};
	};
	let (used_percent, resets_at, error_code) = match observation.disposition {
		AccountQuotaDisposition::Unknown => return Err(LocalAccountTransferError::InvalidInput),
		AccountQuotaDisposition::Current(window) | AccountQuotaDisposition::Stale(window) => {
			if window.duration_minutes != observation.duration_minutes {
				return Err(LocalAccountTransferError::InvalidInput);
			}
			(Some(i64::from(window.used_percent)), Some(window.resets_at_unix_micros), None)
		},
		AccountQuotaDisposition::Error(error) => (None, None, Some(quota_error_text(error))),
	};
	connection
		.execute(
			"INSERT INTO account_quota_facts (
			   account_id, duration_minutes, used_percent, resets_at_micros, error_code,
			   observed_at_micros
			 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
			params![
				account_id,
				i64::from(observation.duration_minutes),
				used_percent,
				resets_at,
				error_code,
				observed_at,
			],
		)
		.map_err(|error| LocalAccountTransferError::Database(sqlite_error(error)))?;
	Ok(())
}

fn validate_batch(batch: &LocalAccountTransferBatch) -> Result<(), LocalAccountTransferError> {
	if batch.accounts.is_empty()
		|| batch.accounts.len() > MAX_ACCOUNTS
		|| !is_sha256(&batch.source_sha256)
		|| batch.routing.revision < 1
		|| batch.routing.order.len() != batch.accounts.len()
	{
		return Err(LocalAccountTransferError::InvalidInput);
	}
	let mut account_ids = BTreeSet::new();
	let mut providers = BTreeSet::new();
	let mut writers = BTreeSet::new();
	for transferred in &batch.accounts {
		let account = &transferred.account;
		let Some(binding) = account.credential.as_ref() else {
			return Err(LocalAccountTransferError::InvalidInput);
		};
		if account.label.is_empty()
			|| account.label.len() > 128
			|| account.label.chars().any(char::is_control)
			|| account.revision < 1
			|| account.tombstoned
			|| account.unsettled_operation.is_some()
			|| transferred.credential.payload.is_empty()
			|| transferred.credential.payload.len() > 1024 * 1024
			|| !account_ids.insert(account.account_id.as_str())
			|| !providers.insert(binding.provider.account_id())
			|| !writers.insert(binding.writer_operation_id.as_str())
			|| !credential_matches(account.account_id.as_str(), binding, &transferred.credential)
		{
			return Err(LocalAccountTransferError::InvalidInput);
		}
	}
	let routing_ids =
		batch.routing.order.iter().map(|value| value.as_str()).collect::<BTreeSet<_>>();
	if routing_ids != account_ids {
		return Err(LocalAccountTransferError::InvalidInput);
	}
	if let AccountSelectionMode::Fixed(account_id) = &batch.routing.mode
		&& !account_ids.contains(account_id.as_str())
	{
		return Err(LocalAccountTransferError::InvalidInput);
	}
	Ok(())
}

fn credential_matches(
	account_id: &str,
	binding: &CredentialBinding,
	record: &CredentialRecord,
) -> bool {
	record.key.account_id == account_id
		&& record.key.schema_version == binding.schema_version.get()
		&& record.key.credential_version == binding.version.get()
		&& record.key.fingerprint == binding.fingerprint.as_str()
		&& record.key.writer_operation_id == binding.writer_operation_id.as_str()
		&& record.key.provider == provider_text(binding.provider.provider())
		&& record.key.provider_account_id == binding.provider.account_id()
}

fn is_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

const fn provider_text(provider: AccountProvider) -> &'static str {
	match provider {
		AccountProvider::Chatgpt => "chatgpt",
	}
}

const fn state_text(state: AccountState) -> &'static str {
	match state {
		AccountState::Unavailable => "unavailable",
		AccountState::Unknown => "unknown",
		AccountState::Available => "available",
		AccountState::Depleted => "depleted",
		AccountState::AuthFailed => "auth_failed",
		AccountState::PluginUnready => "plugin_unready",
	}
}

const fn quota_error_text(error: AccountQuotaObservationError) -> &'static str {
	match error {
		AccountQuotaObservationError::ProviderUnavailable => "provider_unavailable",
		AccountQuotaObservationError::ProtocolUnavailable => "protocol_unavailable",
		AccountQuotaObservationError::AccountMismatch => "account_mismatch",
		AccountQuotaObservationError::UnsupportedWindow => "unsupported_window",
	}
}
