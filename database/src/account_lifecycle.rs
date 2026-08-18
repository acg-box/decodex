//! Account registry, credential-negative lifecycle, and routing control.

use std::collections::BTreeSet;

use decodex_core::{
	AccountId, AccountLifecycleReadiness, AccountOperation, AccountOperationId,
	AccountOperationKind, AccountOperationPhase, AccountOperationStatus, AccountProvider,
	AccountQuotaDisposition, AccountQuotaObservationError, AccountQuotaWindow,
	AccountQuotaWindowObservation, AccountRecord, AccountRoutingControl, AccountSelectionMode,
	AccountState, CredentialBinding, CredentialFingerprint, CredentialStoreSchemaVersion,
	CredentialVersion, ProviderIdentity,
};
use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};
use serde_json::{Value, json};

use crate::{CommandIdentity, DatabaseError, SqliteStore, StoreError, unix_micros};

const ACCOUNT_COMMAND_PROTOCOL: &str = "decodex/account-command/1";
const CLAIM_LIFETIME_MICROS: i64 = 5 * 60 * 1_000_000;
const QUOTA_FRESHNESS_MICROS: i64 = 5 * 60 * 1_000_000;
const MAX_ACCOUNT_COUNT: usize = 512;
const MAX_UNSETTLED_ACCOUNT_OPERATION_COUNT: usize = MAX_ACCOUNT_COUNT * 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountOperationPreparation {
	pub operation_id: AccountOperationId,
	pub account_id: AccountId,
	pub kind: AccountOperationKind,
	pub display_label: Option<String>,
	pub enabled: Option<bool>,
	pub expected_account_revision: Option<i64>,
	pub expected: Option<CredentialBinding>,
	pub target: Option<CredentialBinding>,
	pub provider: ProviderIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountLifecycleMutation {
	pub account_revision: i64,
	pub phase: AccountOperationPhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountLifecycleRejection {
	IdentityConflict,
	OperationUnsettled,
	InvalidRequest,
	AccountMissing,
	StaleAccount,
	AccountInUse,
	OperationMissing,
	StaleOperation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountLifecycleMutationOutcome {
	Applied(AccountLifecycleMutation),
	Replayed(AccountLifecycleMutation),
	Rejected { rejection: AccountLifecycleRejection, actual: AccountLifecycleMutation },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountAdministrationOutcome {
	Updated { revision: i64 },
	Rejected { rejection: AccountLifecycleRejection, revision: i64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoutingControlOutcome {
	Updated { routing: AccountRoutingControl },
	StaleRoutingControl { revision: i64 },
	StaleAccount { revision: i64 },
	AccountMissing,
	InvalidOrder { revision: i64 },
	InvalidRequest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountStoreObservation {
	Exact,
	Missing,
	Mismatch,
	ProviderMismatch,
	Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexAccountCapabilityAttestation {
	pub build_identity: String,
	pub executable_sha256: String,
	pub schema_sha256: String,
	pub callback_profile_sha256: String,
	pub login_chatgpt_auth_tokens: bool,
	pub refresh_callback: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountCommandKind {
	Enroll,
	Import,
	SetEnabled,
	UseInCodex,
	Logout,
	SetFixedSelection,
	SetBalancedSelection,
	SetAccountOrder,
	Refresh,
	Recover,
}

impl AccountCommandKind {
	const fn as_str(self) -> &'static str {
		match self {
			Self::Enroll => "enroll_account",
			Self::Import => "import_account_credential_file",
			Self::SetEnabled => "set_account_enabled",
			Self::UseInCodex => "use_account_in_codex",
			Self::Logout => "logout_account",
			Self::SetFixedSelection => "set_fixed_account_selection",
			Self::SetBalancedSelection => "set_balanced_account_selection",
			Self::SetAccountOrder => "set_account_order",
			Self::Refresh => "refresh_account",
			Self::Recover => "recover_account_operation",
		}
	}
}

pub struct AccountCommandReceiptLease(CommandReservation);

pub enum AccountCommandReceiptClaim {
	Owned(AccountCommandReceiptLease),
	Replayed(Value),
}

struct CommandReservation {
	protocol: &'static str,
	key: String,
	request_hash: String,
	claim_token: String,
}

#[derive(Clone)]
struct AccountBase {
	account_id: String,
	label: String,
	enabled: bool,
	state: String,
	revision: i64,
	provider: String,
	provider_account_id: String,
	store_observation: String,
	tombstoned: bool,
}

impl SqliteStore {
	pub async fn read_account_registry_snapshot(
		&self,
		limit: u16,
	) -> Result<(Vec<AccountRecord>, AccountRoutingControl), StoreError> {
		validate_limit(limit, "account registry limit must be between 1 and 512")?;
		self.run(move |connection| {
			let accounts = read_account_registry_sync(connection, None, limit)?;
			let routing = read_routing_control_sync(connection)?;
			validate_registry_snapshot(&accounts, &routing)?;
			Ok((accounts, routing))
		})
		.await
	}

	pub async fn reserve_account_command(
		&self,
		command: &CommandIdentity,
		kind: AccountCommandKind,
		entity_id: &str,
		expected_revision: Option<i64>,
	) -> Result<AccountCommandReceiptClaim, StoreError> {
		if entity_id.is_empty() || entity_id.len() > 256 || entity_id.chars().any(char::is_control)
		{
			return Err(StoreError::InvalidInput("account command entity identity is invalid"));
		}
		if expected_revision.is_some_and(|revision| revision < 1) {
			return Err(StoreError::InvalidInput("account command revision must be positive"));
		}
		let command = command.clone();
		let entity_id = entity_id.to_owned();
		self.run(move |connection| {
			reserve_command_sync(connection, command, kind, entity_id, expected_revision)
		})
		.await
	}

	pub async fn complete_account_command(
		&self,
		lease: AccountCommandReceiptLease,
		result: &Value,
	) -> Result<(), StoreError> {
		validate_account_command_response(result)?;
		let result = result.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			finish_command_sync(&transaction, &lease.0, &result)?;
			transaction.commit().map_err(sql_error)
		})
		.await
	}

	pub async fn read_account_registry(
		&self,
		account_id: Option<&AccountId>,
		limit: u16,
	) -> Result<Vec<AccountRecord>, StoreError> {
		validate_limit(limit, "account registry limit must be between 1 and 512")?;
		let account_id = account_id.map(|value| value.as_str().to_owned());
		self.run(move |connection| {
			read_account_registry_sync(connection, account_id.as_deref(), limit)
		})
		.await
	}

	pub async fn prepare_account_operation(
		&self,
		preparation: &AccountOperationPreparation,
	) -> Result<AccountLifecycleMutationOutcome, StoreError> {
		validate_preparation(preparation)?;
		let preparation = preparation.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let outcome = prepare_operation_sync(&transaction, &preparation, None)?;
			transaction.commit().map_err(sql_error)?;
			Ok(outcome)
		})
		.await
	}

	/// Prepare a verified credential replacement beside one exact ambiguous refresh.
	pub async fn prepare_account_reauthentication_takeover(
		&self,
		preparation: &AccountOperationPreparation,
		recovery_operation_id: &AccountOperationId,
	) -> Result<AccountLifecycleMutationOutcome, StoreError> {
		validate_preparation(preparation)?;
		if preparation.operation_id == *recovery_operation_id {
			return Err(StoreError::InvalidInput(
				"account reauthentication recovery identity is invalid",
			));
		}
		let preparation = preparation.clone();
		let recovery_operation_id = recovery_operation_id.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let outcome =
				prepare_operation_sync(&transaction, &preparation, Some(&recovery_operation_id))?;
			transaction.commit().map_err(sql_error)?;
			Ok(outcome)
		})
		.await
	}

	pub async fn advance_account_operation(
		&self,
		operation_id: &AccountOperationId,
		expected: AccountOperationPhase,
		target: AccountOperationPhase,
		recovery_code: Option<&str>,
	) -> Result<AccountLifecycleMutationOutcome, StoreError> {
		validate_recovery_code(recovery_code)?;
		let operation_id = operation_id.clone();
		let recovery_code = recovery_code.map(str::to_owned);
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let outcome = advance_operation_sync(
				&transaction,
				&operation_id,
				expected,
				target,
				recovery_code.as_deref(),
			)?;
			transaction.commit().map_err(sql_error)?;
			Ok(outcome)
		})
		.await
	}

	pub async fn complete_account_operation_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: &AccountOperationId,
		expected: AccountOperationPhase,
		target: AccountOperationPhase,
		recovery_code: Option<&str>,
		build_response: F,
	) -> Result<Value, StoreError>
	where
		F: FnOnce(
				&AccountLifecycleMutationOutcome,
				Option<&AccountOperation>,
				Option<&AccountRecord>,
			) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		validate_recovery_code(recovery_code)?;
		let operation_id = operation_id.clone();
		let recovery_code = recovery_code.map(str::to_owned);
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let outcome = advance_operation_sync(
				&transaction,
				&operation_id,
				expected,
				target,
				recovery_code.as_deref(),
			)?;
			let operation = read_operation_sync(&transaction, &operation_id)?;
			let account = match operation.as_ref() {
				Some(operation) => read_account_registry_sync(
					&transaction,
					Some(operation.account_id.as_str()),
					1,
				)?
				.into_iter()
				.next(),
				None => None,
			};
			let response = build_response(&outcome, operation.as_ref(), account.as_ref())?;
			validate_account_command_response(&response)?;
			finish_command_sync(&transaction, &lease.0, &response)?;
			transaction.commit().map_err(sql_error)?;
			Ok(response)
		})
		.await
	}

	pub async fn set_account_operation_target(
		&self,
		operation_id: &AccountOperationId,
		target: &CredentialBinding,
	) -> Result<AccountLifecycleMutationOutcome, StoreError> {
		let operation_id = operation_id.clone();
		let target = target.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let outcome = set_operation_target_sync(&transaction, &operation_id, &target)?;
			transaction.commit().map_err(sql_error)?;
			Ok(outcome)
		})
		.await
	}

	pub async fn read_unsettled_account_operations(
		&self,
		limit: u16,
	) -> Result<Vec<AccountOperation>, StoreError> {
		validate_bounded_limit(
			limit,
			MAX_UNSETTLED_ACCOUNT_OPERATION_COUNT,
			"account operation limit must be between 1 and 1024",
		)?;
		self.run(move |connection| {
			let mut statement = connection
				.prepare(
					"SELECT operation_id FROM account_operations
					 WHERE phase NOT IN ('committed', 'cancelled')
					   AND superseded_by_operation_id IS NULL
					 ORDER BY recovery_operation_id IS NULL, created_at_micros, operation_id
					 LIMIT ?1",
				)
				.map_err(sql_error)?;
			let rows = statement
				.query_map(params![i64::from(limit)], |row| row.get::<_, String>(0))
				.map_err(sql_error)?;
			let ids = rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)?;
			ids.into_iter()
				.map(|id| {
					let id = AccountOperationId::new(id)
						.map_err(|_| incompatible("account operation identity"))?;
					read_operation_sync(connection, &id)?
						.ok_or_else(|| incompatible("account operation readback"))
				})
				.collect()
		})
		.await
	}

	pub async fn read_account_operation(
		&self,
		operation_id: &AccountOperationId,
	) -> Result<Option<AccountOperation>, StoreError> {
		let operation_id = operation_id.clone();
		self.run(move |connection| read_operation_sync(connection, &operation_id)).await
	}

	pub async fn set_account_enabled(
		&self,
		account_id: &AccountId,
		expected_revision: i64,
		enabled: bool,
	) -> Result<AccountAdministrationOutcome, StoreError> {
		validate_account_revision(expected_revision)?;
		let account_id = account_id.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let outcome =
				set_account_enabled_sync(&transaction, &account_id, expected_revision, enabled)?;
			transaction.commit().map_err(sql_error)?;
			Ok(outcome)
		})
		.await
	}

	pub async fn set_account_enabled_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		account_id: &AccountId,
		expected_revision: i64,
		enabled: bool,
		build_response: F,
	) -> Result<Value, StoreError>
	where
		F: FnOnce(
				&AccountAdministrationOutcome,
				Option<&AccountRecord>,
			) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		validate_account_revision(expected_revision)?;
		let account_id = account_id.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let outcome =
				set_account_enabled_sync(&transaction, &account_id, expected_revision, enabled)?;
			let account = if matches!(outcome, AccountAdministrationOutcome::Updated { .. }) {
				read_account_registry_sync(&transaction, Some(account_id.as_str()), 1)?
					.into_iter()
					.next()
			} else {
				None
			};
			let response = build_response(&outcome, account.as_ref())?;
			validate_account_command_response(&response)?;
			finish_command_sync(&transaction, &lease.0, &response)?;
			transaction.commit().map_err(sql_error)?;
			Ok(response)
		})
		.await
	}

	pub async fn read_account_routing_control(&self) -> Result<AccountRoutingControl, StoreError> {
		self.run(|connection| read_routing_control_sync(connection)).await
	}

	pub async fn set_fixed_account_selection(
		&self,
		expected_routing_revision: i64,
		account_id: &AccountId,
		expected_account_revision: i64,
	) -> Result<RoutingControlOutcome, StoreError> {
		validate_routing_revision(expected_routing_revision)?;
		validate_account_revision(expected_account_revision)?;
		let account_id = account_id.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let outcome = set_fixed_routing_sync(
				&transaction,
				expected_routing_revision,
				&account_id,
				expected_account_revision,
			)?;
			transaction.commit().map_err(sql_error)?;
			Ok(outcome)
		})
		.await
	}

	pub async fn set_fixed_account_selection_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		expected_routing_revision: i64,
		account_id: &AccountId,
		expected_account_revision: i64,
		build_response: F,
	) -> Result<Value, StoreError>
	where
		F: FnOnce(&RoutingControlOutcome) -> Result<Value, StoreError> + Send + 'static,
	{
		validate_routing_revision(expected_routing_revision)?;
		validate_account_revision(expected_account_revision)?;
		let account_id = account_id.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let outcome = set_fixed_routing_sync(
				&transaction,
				expected_routing_revision,
				&account_id,
				expected_account_revision,
			)?;
			let response = build_response(&outcome)?;
			validate_account_command_response(&response)?;
			finish_command_sync(&transaction, &lease.0, &response)?;
			transaction.commit().map_err(sql_error)?;
			Ok(response)
		})
		.await
	}

	pub async fn set_balanced_account_selection(
		&self,
		expected_routing_revision: i64,
	) -> Result<RoutingControlOutcome, StoreError> {
		validate_routing_revision(expected_routing_revision)?;
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let outcome = set_balanced_routing_sync(&transaction, expected_routing_revision)?;
			transaction.commit().map_err(sql_error)?;
			Ok(outcome)
		})
		.await
	}

	pub async fn set_balanced_account_selection_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		expected_routing_revision: i64,
		build_response: F,
	) -> Result<Value, StoreError>
	where
		F: FnOnce(&RoutingControlOutcome) -> Result<Value, StoreError> + Send + 'static,
	{
		validate_routing_revision(expected_routing_revision)?;
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let outcome = set_balanced_routing_sync(&transaction, expected_routing_revision)?;
			let response = build_response(&outcome)?;
			validate_account_command_response(&response)?;
			finish_command_sync(&transaction, &lease.0, &response)?;
			transaction.commit().map_err(sql_error)?;
			Ok(response)
		})
		.await
	}

	pub async fn set_account_order(
		&self,
		expected_routing_revision: i64,
		order: &[AccountId],
	) -> Result<RoutingControlOutcome, StoreError> {
		validate_routing_revision(expected_routing_revision)?;
		validate_order(order)?;
		let order = order.to_vec();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let outcome = set_account_order_sync(&transaction, expected_routing_revision, &order)?;
			transaction.commit().map_err(sql_error)?;
			Ok(outcome)
		})
		.await
	}

	pub async fn set_account_order_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		expected_routing_revision: i64,
		order: &[AccountId],
		build_response: F,
	) -> Result<Value, StoreError>
	where
		F: FnOnce(&RoutingControlOutcome) -> Result<Value, StoreError> + Send + 'static,
	{
		validate_routing_revision(expected_routing_revision)?;
		validate_order(order)?;
		let order = order.to_vec();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let outcome = set_account_order_sync(&transaction, expected_routing_revision, &order)?;
			let response = build_response(&outcome)?;
			validate_account_command_response(&response)?;
			finish_command_sync(&transaction, &lease.0, &response)?;
			transaction.commit().map_err(sql_error)?;
			Ok(response)
		})
		.await
	}

	pub async fn observe_account_quota(
		&self,
		account_id: &AccountId,
		fact: AccountQuotaWindow,
		observed_at_unix_micros: i64,
	) -> Result<(), StoreError> {
		if observed_at_unix_micros < 0 || fact.resets_at_unix_micros <= observed_at_unix_micros {
			return Err(StoreError::InvalidInput("quota fact rejected"));
		}
		let account_id = account_id.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let changed = transaction
				.execute(
					"INSERT INTO account_quota_facts (
					   account_id, duration_minutes, used_percent, resets_at_micros,
					   error_code, observed_at_micros
					 ) VALUES (?1, ?2, ?3, ?4, NULL, ?5)
					 ON CONFLICT(account_id, duration_minutes) DO UPDATE SET
					   used_percent = excluded.used_percent,
					   resets_at_micros = excluded.resets_at_micros,
					   error_code = NULL,
					   observed_at_micros = excluded.observed_at_micros
					 WHERE excluded.observed_at_micros >= account_quota_facts.observed_at_micros",
					params![
						account_id.as_str(),
						i64::from(fact.duration_minutes),
						i64::from(fact.used_percent),
						fact.resets_at_unix_micros,
						observed_at_unix_micros,
					],
				)
				.map_err(sql_error)?;
			if changed == 0 {
				return Err(StoreError::InvalidInput("quota fact rejected"));
			}
			let account_changed = transaction
				.execute(
					"UPDATE accounts SET state = CASE WHEN EXISTS (
					   SELECT 1 FROM account_quota_facts
					   WHERE account_id = ?1 AND error_code IS NULL AND used_percent >= 100
					 ) THEN 'depleted' ELSE 'available' END,
					 updated_at_micros = ?2
					 WHERE account_id = ?1 AND tombstoned_at_micros IS NULL",
					params![account_id.as_str(), unix_micros().map_err(StoreError::from)?],
				)
				.map_err(sql_error)?;
			if account_changed != 1 {
				return Err(StoreError::InvalidInput("quota fact rejected"));
			}
			transaction.commit().map_err(sql_error)
		})
		.await
	}

	pub async fn observe_account_quota_error(
		&self,
		account_id: &AccountId,
		duration_minutes: u32,
		error: AccountQuotaObservationError,
		observed_at_unix_micros: i64,
	) -> Result<(), StoreError> {
		if !matches!(duration_minutes, 300 | 10_080) || observed_at_unix_micros < 0 {
			return Err(StoreError::InvalidInput("quota error rejected"));
		}
		let account_id = account_id.clone();
		self.run(move |connection| {
			let changed = connection
				.execute(
					"INSERT INTO account_quota_facts (
					   account_id, duration_minutes, used_percent, resets_at_micros,
					   error_code, observed_at_micros
					 ) VALUES (?1, ?2, NULL, NULL, ?3, ?4)
					 ON CONFLICT(account_id, duration_minutes) DO UPDATE SET
					   used_percent = NULL,
					   resets_at_micros = NULL,
					   error_code = excluded.error_code,
					   observed_at_micros = excluded.observed_at_micros
					 WHERE excluded.observed_at_micros >= account_quota_facts.observed_at_micros",
					params![
						account_id.as_str(),
						i64::from(duration_minutes),
						quota_error_text(error),
						observed_at_unix_micros,
					],
				)
				.map_err(sql_error)?;
			if changed > 0 { Ok(()) } else { Err(StoreError::InvalidInput("quota error rejected")) }
		})
		.await
	}

	pub async fn observe_account_store(
		&self,
		account_id: &AccountId,
		expected_revision: i64,
		expected: &CredentialBinding,
		observation: AccountStoreObservation,
	) -> Result<bool, StoreError> {
		validate_account_revision(expected_revision)?;
		let account_id = account_id.clone();
		let expected = expected.clone();
		self.run(move |connection| {
			let revision = account_revision_sync(connection, &account_id)?;
			if revision != expected_revision
				|| credential_binding_sync(connection, &account_id)?.as_ref() != Some(&expected)
			{
				return Ok(false);
			}
			let changed = connection
				.execute(
					"UPDATE accounts SET credential_store_observation = ?1,
					 updated_at_micros = ?2 WHERE account_id = ?3 AND revision = ?4",
					params![
						store_observation_text(observation),
						unix_micros().map_err(StoreError::from)?,
						account_id.as_str(),
						expected_revision,
					],
				)
				.map_err(sql_error)?;
			Ok(changed == 1)
		})
		.await
	}

	pub async fn attest_codex_account_capability(
		&self,
		attestation: &CodexAccountCapabilityAttestation,
	) -> Result<bool, StoreError> {
		if attestation.build_identity.is_empty()
			|| attestation.build_identity.len() > 256
			|| ![
				attestation.executable_sha256.as_str(),
				attestation.schema_sha256.as_str(),
				attestation.callback_profile_sha256.as_str(),
			]
			.into_iter()
			.all(is_sha256)
		{
			return Err(StoreError::InvalidInput("Codex account capability is invalid"));
		}
		let attestation = attestation.clone();
		self.run(move |connection| {
			connection
				.execute(
					"INSERT INTO codex_account_capability (
					   singleton, build_identity, executable_sha256, schema_sha256, callback_profile_sha256,
					   login_chatgpt_auth_tokens, refresh_callback, observed_at_micros
					 ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)
					 ON CONFLICT(singleton) DO UPDATE SET
					   build_identity = excluded.build_identity,
					   executable_sha256 = excluded.executable_sha256,
					   schema_sha256 = excluded.schema_sha256,
					   callback_profile_sha256 = excluded.callback_profile_sha256,
					   login_chatgpt_auth_tokens = excluded.login_chatgpt_auth_tokens,
					   refresh_callback = excluded.refresh_callback,
					   observed_at_micros = excluded.observed_at_micros",
					params![
						attestation.build_identity,
						attestation.executable_sha256,
						attestation.schema_sha256,
						attestation.callback_profile_sha256,
						attestation.login_chatgpt_auth_tokens,
						attestation.refresh_callback,
						unix_micros().map_err(StoreError::from)?,
					],
				)
				.map_err(sql_error)?;
			Ok(attestation.login_chatgpt_auth_tokens && attestation.refresh_callback)
		})
		.await
	}
}

fn reserve_command_sync(
	connection: &mut Connection,
	command: CommandIdentity,
	kind: AccountCommandKind,
	entity_id: String,
	expected_revision: Option<i64>,
) -> Result<AccountCommandReceiptClaim, StoreError> {
	let transaction =
		connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql_error)?;
	let now = unix_micros().map_err(StoreError::from)?;
	let existing = transaction
		.query_row(
			"SELECT request_sha256, operation, entity_id, expected_revision, state,
			        response_json, claim_expires_at_micros
			 FROM command_receipts WHERE protocol = ?1 AND idempotency_key = ?2",
			params![ACCOUNT_COMMAND_PROTOCOL, command.key],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, Option<i64>>(3)?,
					row.get::<_, String>(4)?,
					row.get::<_, Option<String>>(5)?,
					row.get::<_, Option<i64>>(6)?,
				))
			},
		)
		.optional()
		.map_err(sql_error)?;
	let receipt_exists = existing.is_some();
	if let Some((request, operation, stored_entity, revision, state, response, expires)) = existing
	{
		if request != command.request_hash
			|| operation != kind.as_str()
			|| stored_entity != entity_id
			|| revision != expected_revision
		{
			return Err(StoreError::IdempotencyConflict);
		}
		if state != "reserved" {
			let response = response.ok_or_else(|| incompatible("command response"))?;
			let response =
				serde_json::from_str(&response).map_err(|_| incompatible("command response"))?;
			transaction.commit().map_err(sql_error)?;
			return Ok(AccountCommandReceiptClaim::Replayed(response));
		}
		if expires.is_some_and(|expires| expires > now) {
			return Err(StoreError::OwnershipLost("command receipt claim is active"));
		}
	}

	let claim_token = random_uuid_v4()?;
	let expires = now
		.checked_add(CLAIM_LIFETIME_MICROS)
		.ok_or(StoreError::InvalidInput("command claim timestamp is invalid"))?;
	if receipt_exists {
		transaction
			.execute(
				"UPDATE command_receipts SET claim_token = ?1, claim_expires_at_micros = ?2
				 WHERE protocol = ?3 AND idempotency_key = ?4 AND state = 'reserved'",
				params![claim_token, expires, ACCOUNT_COMMAND_PROTOCOL, command.key],
			)
			.map_err(sql_error)?;
	} else {
		transaction
			.execute(
				"INSERT INTO command_receipts (
				   protocol, idempotency_key, request_sha256, operation, entity_id,
				   expected_revision, state, response_json, claim_token,
				   claim_expires_at_micros, reserved_at_micros, completed_at_micros
				 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'reserved', NULL, ?7, ?8, ?9, NULL)",
				params![
					ACCOUNT_COMMAND_PROTOCOL,
					command.key,
					command.request_hash,
					kind.as_str(),
					entity_id,
					expected_revision,
					claim_token,
					expires,
					now,
				],
			)
			.map_err(sql_error)?;
	}
	transaction.commit().map_err(sql_error)?;
	Ok(AccountCommandReceiptClaim::Owned(AccountCommandReceiptLease(CommandReservation {
		protocol: ACCOUNT_COMMAND_PROTOCOL,
		key: command.key,
		request_hash: command.request_hash,
		claim_token,
	})))
}

fn finish_command_sync(
	connection: &Connection,
	reservation: &CommandReservation,
	response: &Value,
) -> Result<(), StoreError> {
	let response = serde_json::to_string(response)
		.map_err(|_| StoreError::InvalidInput("account command result is invalid"))?;
	let changed = connection
		.execute(
			"UPDATE command_receipts
			 SET state = 'completed_success', response_json = ?1, claim_token = NULL,
			     claim_expires_at_micros = NULL, completed_at_micros = ?2
			 WHERE protocol = ?3 AND idempotency_key = ?4 AND request_sha256 = ?5
			   AND state = 'reserved' AND claim_token = ?6 AND claim_expires_at_micros > ?2",
			params![
				response,
				unix_micros().map_err(StoreError::from)?,
				reservation.protocol,
				reservation.key,
				reservation.request_hash,
				reservation.claim_token,
			],
		)
		.map_err(sql_error)?;
	if changed == 1 { Ok(()) } else { Err(StoreError::OwnershipLost("command receipt claim")) }
}

fn prepare_operation_sync(
	connection: &Connection,
	preparation: &AccountOperationPreparation,
	recovery_operation_id: Option<&AccountOperationId>,
) -> Result<AccountLifecycleMutationOutcome, StoreError> {
	if let Some(existing) = read_operation_sync(connection, &preparation.operation_id)? {
		let descriptor_matches = existing.account_id == preparation.account_id
			&& existing.kind == preparation.kind
			&& existing.expected_account_revision == preparation.expected_account_revision
			&& existing.requested_display_label == preparation.display_label
			&& existing.requested_enabled == preparation.enabled
			&& existing.expected == preparation.expected
			&& existing.target == preparation.target
			&& existing.recovery_operation_id.as_ref() == recovery_operation_id;
		let actual = mutation_for(connection, &existing.account_id, existing.phase)?;
		return if descriptor_matches {
			Ok(AccountLifecycleMutationOutcome::Replayed(actual))
		} else {
			Ok(AccountLifecycleMutationOutcome::Rejected {
				rejection: AccountLifecycleRejection::IdentityConflict,
				actual,
			})
		};
	}

	if let Some(recovery_operation_id) = recovery_operation_id {
		if let Some(rejection) =
			validate_reauthentication_takeover_sync(connection, preparation, recovery_operation_id)?
		{
			return Ok(rejection);
		}
	} else if let Some((phase, account_id)) = connection
		.query_row(
			"SELECT phase, account_id FROM account_operations
			 WHERE account_id = ?1 AND phase NOT IN ('committed', 'cancelled')
			   AND superseded_by_operation_id IS NULL LIMIT 1",
			params![preparation.account_id.as_str()],
			|row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
		)
		.optional()
		.map_err(sql_error)?
	{
		let account_id =
			AccountId::new(account_id).map_err(|_| incompatible("account identity"))?;
		return Ok(AccountLifecycleMutationOutcome::Rejected {
			rejection: AccountLifecycleRejection::OperationUnsettled,
			actual: mutation_for(connection, &account_id, parse_operation_phase(&phase)?)?,
		});
	}

	let account = account_base_sync(connection, &preparation.account_id)?;
	let rejection = match preparation.kind {
		AccountOperationKind::Enroll | AccountOperationKind::Import if account.is_some() =>
			Some(AccountLifecycleRejection::IdentityConflict),
		AccountOperationKind::Enroll | AccountOperationKind::Import => None,
		AccountOperationKind::Refresh | AccountOperationKind::Logout => match account {
			None => Some(AccountLifecycleRejection::AccountMissing),
			Some(ref account) if account.tombstoned =>
				Some(AccountLifecycleRejection::AccountMissing),
			Some(ref account)
				if preparation.expected_account_revision != Some(account.revision) =>
				Some(AccountLifecycleRejection::StaleAccount),
			Some(_)
				if credential_binding_sync(connection, &preparation.account_id)?
					!= preparation.expected =>
				Some(AccountLifecycleRejection::StaleAccount),
			Some(_) => None,
		},
	};
	if let Some(rejection) = rejection {
		return Ok(AccountLifecycleMutationOutcome::Rejected {
			rejection,
			actual: AccountLifecycleMutation {
				account_revision: account.as_ref().map_or(0, |value| value.revision),
				phase: AccountOperationPhase::Prepared,
			},
		});
	}

	let now = unix_micros().map_err(StoreError::from)?;
	connection
		.execute(
			"INSERT OR IGNORE INTO account_identities (account_id, created_at_micros)
			 VALUES (?1, ?2)",
			params![preparation.account_id.as_str(), now],
		)
		.map_err(sql_error)?;
	connection
		.execute(
			"INSERT INTO account_operations (
			   operation_id, account_id, kind, phase, expected_account_revision,
			   expected_credential_json, target_credential_json, provider,
			   provider_account_id, requested_display_label, requested_enabled,
			   recovery_code, created_at_micros, updated_at_micros, completed_at_micros,
			   recovery_operation_id, superseded_by_operation_id
			 ) VALUES (
			   ?1, ?2, ?3, 'prepared', ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?11,
			   NULL, ?12, NULL
			 )",
			params![
				preparation.operation_id.as_str(),
				preparation.account_id.as_str(),
				operation_kind_text(preparation.kind),
				preparation.expected_account_revision,
				binding_json(preparation.expected.as_ref())?,
				binding_json(preparation.target.as_ref())?,
				provider_text(preparation.provider.provider()),
				preparation.provider.account_id(),
				preparation.display_label,
				preparation.enabled,
				now,
				recovery_operation_id.map(AccountOperationId::as_str),
			],
		)
		.map_err(sql_error)?;
	Ok(AccountLifecycleMutationOutcome::Applied(AccountLifecycleMutation {
		account_revision: account.map_or(0, |value| value.revision),
		phase: AccountOperationPhase::Prepared,
	}))
}

fn validate_reauthentication_takeover_sync(
	connection: &Connection,
	preparation: &AccountOperationPreparation,
	recovery_operation_id: &AccountOperationId,
) -> Result<Option<AccountLifecycleMutationOutcome>, StoreError> {
	let Some(recovery) = read_operation_sync(connection, recovery_operation_id)? else {
		return Ok(Some(AccountLifecycleMutationOutcome::Rejected {
			rejection: AccountLifecycleRejection::OperationMissing,
			actual: AccountLifecycleMutation {
				account_revision: 0,
				phase: AccountOperationPhase::RecoveryRequired,
			},
		}));
	};
	let actual = mutation_for(connection, &recovery.account_id, recovery.phase)?;
	let exact_ambiguity = preparation.kind == AccountOperationKind::Refresh
		&& preparation.target.is_some()
		&& recovery.account_id == preparation.account_id
		&& recovery.kind == AccountOperationKind::Refresh
		&& recovery.phase == AccountOperationPhase::RecoveryRequired
		&& recovery.target.is_none()
		&& recovery.recovery_code.is_some()
		&& recovery.recovery_operation_id.is_none()
		&& recovery.superseded_by_operation_id.is_none()
		&& recovery.expected_account_revision == preparation.expected_account_revision
		&& recovery.expected == preparation.expected;
	if !exact_ambiguity {
		return Ok(Some(AccountLifecycleMutationOutcome::Rejected {
			rejection: AccountLifecycleRejection::StaleOperation,
			actual,
		}));
	}
	let active_takeover = connection
		.query_row(
			"SELECT phase FROM account_operations
			 WHERE recovery_operation_id = ?1
			   AND phase NOT IN ('committed', 'cancelled') LIMIT 1",
			params![recovery_operation_id.as_str()],
			|row| row.get::<_, String>(0),
		)
		.optional()
		.map_err(sql_error)?;
	if let Some(phase) = active_takeover {
		return Ok(Some(AccountLifecycleMutationOutcome::Rejected {
			rejection: AccountLifecycleRejection::OperationUnsettled,
			actual: AccountLifecycleMutation {
				account_revision: actual.account_revision,
				phase: parse_operation_phase(&phase)?,
			},
		}));
	}
	Ok(None)
}

fn advance_operation_sync(
	connection: &Connection,
	operation_id: &AccountOperationId,
	expected: AccountOperationPhase,
	target: AccountOperationPhase,
	recovery_code: Option<&str>,
) -> Result<AccountLifecycleMutationOutcome, StoreError> {
	validate_transition_recovery_shape(target, recovery_code)?;
	let Some(operation) = read_operation_sync(connection, operation_id)? else {
		return Ok(AccountLifecycleMutationOutcome::Rejected {
			rejection: AccountLifecycleRejection::OperationMissing,
			actual: AccountLifecycleMutation { account_revision: 0, phase: expected },
		});
	};
	let actual = mutation_for(connection, &operation.account_id, operation.phase)?;
	if operation.phase == target {
		return Ok(AccountLifecycleMutationOutcome::Replayed(actual));
	}
	if operation.superseded_by_operation_id.is_some()
		|| operation.phase != expected
		|| !allowed_operation_transition(expected, target)
	{
		return Ok(AccountLifecycleMutationOutcome::Rejected {
			rejection: AccountLifecycleRejection::StaleOperation,
			actual,
		});
	}
	if target == AccountOperationPhase::Committed
		&& let Some(rejection) = commit_account_operation(connection, &operation)?
	{
		return Ok(AccountLifecycleMutationOutcome::Rejected { rejection, actual });
	}
	let now = unix_micros().map_err(StoreError::from)?;
	let completed =
		matches!(target, AccountOperationPhase::Committed | AccountOperationPhase::Cancelled)
			.then_some(now);
	let changed = connection
		.execute(
			"UPDATE account_operations SET phase = ?1, recovery_code = ?2,
			 updated_at_micros = ?3, completed_at_micros = ?4
			 WHERE operation_id = ?5 AND phase = ?6",
			params![
				operation_phase_text(target),
				recovery_code,
				now,
				completed,
				operation_id.as_str(),
				operation_phase_text(expected),
			],
		)
		.map_err(sql_error)?;
	if changed != 1 {
		return Err(incompatible("account operation transition"));
	}
	Ok(AccountLifecycleMutationOutcome::Applied(mutation_for(
		connection,
		&operation.account_id,
		target,
	)?))
}

#[allow(clippy::too_many_lines)] // Keep every account-operation commit and its takeover linkage in one transaction boundary.
fn commit_account_operation(
	connection: &Connection,
	operation: &AccountOperation,
) -> Result<Option<AccountLifecycleRejection>, StoreError> {
	let now = unix_micros().map_err(StoreError::from)?;
	if let Some(rejection) = validate_reauthentication_takeover_commit(connection, operation)? {
		return Ok(Some(rejection));
	}
	match operation.kind {
		AccountOperationKind::Enroll | AccountOperationKind::Import => {
			let Some(target) = operation.target.as_ref() else {
				return Ok(Some(AccountLifecycleRejection::InvalidRequest));
			};
			if credential_binding_sync(connection, &operation.account_id)?.as_ref() != Some(target)
			{
				return Ok(Some(AccountLifecycleRejection::StaleAccount));
			}
			let label = operation
				.requested_display_label
				.as_ref()
				.ok_or(StoreError::InvalidInput("account label is absent"))?;
			let enabled = operation
				.requested_enabled
				.ok_or(StoreError::InvalidInput("account enablement is absent"))?;
			connection
				.execute(
					"INSERT INTO accounts (
					   account_id, display_label, enabled, state, revision, provider,
					   provider_account_id, credential_store_observation,
					   created_at_micros, updated_at_micros, tombstoned_at_micros
					 ) VALUES (?1, ?2, ?3, 'available', 1, ?4, ?5, 'exact', ?6, ?6, NULL)",
					params![
						operation.account_id.as_str(),
						label,
						enabled,
						provider_text(target.provider.provider()),
						target.provider.account_id(),
						now,
					],
				)
				.map_err(sql_error)?;
			let position: i64 = connection
				.query_row("SELECT COUNT(*) FROM account_routing_order", [], |row| row.get(0))
				.map_err(sql_error)?;
			connection
				.execute(
					"INSERT INTO account_routing_order (account_id, position, updated_at_micros)
					 VALUES (?1, ?2, ?3)",
					params![operation.account_id.as_str(), position, now],
				)
				.map_err(sql_error)?;
			bump_routing_revision(connection, now)?;
		},
		AccountOperationKind::Refresh => {
			let Some(target) = operation.target.as_ref() else {
				return Ok(Some(AccountLifecycleRejection::InvalidRequest));
			};
			if credential_binding_sync(connection, &operation.account_id)?.as_ref() != Some(target)
			{
				return Ok(Some(AccountLifecycleRejection::StaleAccount));
			}
			let changed = connection
				.execute(
					"UPDATE accounts SET revision = revision + 1, provider = ?1,
					 provider_account_id = ?2, credential_store_observation = 'exact',
					 updated_at_micros = ?3
					 WHERE account_id = ?4 AND tombstoned_at_micros IS NULL",
					params![
						provider_text(target.provider.provider()),
						target.provider.account_id(),
						now,
						operation.account_id.as_str(),
					],
				)
				.map_err(sql_error)?;
			if changed != 1 {
				return Ok(Some(AccountLifecycleRejection::AccountMissing));
			}
		},
		AccountOperationKind::Logout => {
			let in_use: bool = connection
				.query_row(
					"SELECT EXISTS (
					   SELECT 1 FROM process_generations WHERE account_id = ?1 AND state <> 'dead'
					 )",
					params![operation.account_id.as_str()],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			if in_use {
				return Ok(Some(AccountLifecycleRejection::AccountInUse));
			}
			if credential_binding_sync(connection, &operation.account_id)?.is_some() {
				return Ok(Some(AccountLifecycleRejection::StaleAccount));
			}
			let changed = connection
				.execute(
					"UPDATE accounts SET enabled = 0, revision = revision + 1,
					 credential_store_observation = 'missing', updated_at_micros = ?1,
					 tombstoned_at_micros = ?1
					 WHERE account_id = ?2 AND tombstoned_at_micros IS NULL",
					params![now, operation.account_id.as_str()],
				)
				.map_err(sql_error)?;
			if changed != 1 {
				return Ok(Some(AccountLifecycleRejection::AccountMissing));
			}
			connection
				.execute(
					"DELETE FROM account_routing_order WHERE account_id = ?1",
					params![operation.account_id.as_str()],
				)
				.map_err(sql_error)?;
			compact_routing_order(connection, now)?;
			connection
				.execute(
					"UPDATE account_routing_control
					 SET mode = CASE WHEN fixed_account_id = ?1 THEN 'balanced' ELSE mode END,
					     fixed_account_id = CASE WHEN fixed_account_id = ?1 THEN NULL ELSE fixed_account_id END
					 WHERE singleton = 1",
					params![operation.account_id.as_str()],
				)
				.map_err(sql_error)?;
			bump_routing_revision(connection, now)?;
		},
	}
	record_reauthentication_supersession(connection, operation, now)?;
	Ok(None)
}

fn validate_reauthentication_takeover_commit(
	connection: &Connection,
	operation: &AccountOperation,
) -> Result<Option<AccountLifecycleRejection>, StoreError> {
	let Some(recovery_operation_id) = operation.recovery_operation_id.as_ref() else {
		return Ok(None);
	};
	let Some(recovery) = read_operation_sync(connection, recovery_operation_id)? else {
		return Ok(Some(AccountLifecycleRejection::OperationMissing));
	};
	let exact_ambiguity = operation.kind == AccountOperationKind::Refresh
		&& recovery.account_id == operation.account_id
		&& recovery.kind == AccountOperationKind::Refresh
		&& recovery.phase == AccountOperationPhase::RecoveryRequired
		&& recovery.target.is_none()
		&& recovery.recovery_code.is_some()
		&& recovery.recovery_operation_id.is_none()
		&& recovery.superseded_by_operation_id.is_none()
		&& recovery.expected_account_revision == operation.expected_account_revision
		&& recovery.expected == operation.expected;
	Ok((!exact_ambiguity).then_some(AccountLifecycleRejection::StaleOperation))
}

fn record_reauthentication_supersession(
	connection: &Connection,
	operation: &AccountOperation,
	now: i64,
) -> Result<(), StoreError> {
	let Some(recovery_operation_id) = operation.recovery_operation_id.as_ref() else {
		return Ok(());
	};
	let changed = connection
		.execute(
			"UPDATE account_operations
			 SET superseded_by_operation_id = ?1, updated_at_micros = ?2
			 WHERE operation_id = ?3 AND phase = 'recovery_required'
			   AND target_credential_json IS NULL
			   AND recovery_operation_id IS NULL
			   AND superseded_by_operation_id IS NULL",
			params![operation.operation_id.as_str(), now, recovery_operation_id.as_str()],
		)
		.map_err(sql_error)?;
	if changed == 1 { Ok(()) } else { Err(incompatible("account reauthentication supersession")) }
}

fn set_operation_target_sync(
	connection: &Connection,
	operation_id: &AccountOperationId,
	target: &CredentialBinding,
) -> Result<AccountLifecycleMutationOutcome, StoreError> {
	let Some(operation) = read_operation_sync(connection, operation_id)? else {
		return Ok(AccountLifecycleMutationOutcome::Rejected {
			rejection: AccountLifecycleRejection::OperationMissing,
			actual: AccountLifecycleMutation {
				account_revision: 0,
				phase: AccountOperationPhase::ProviderEffectPending,
			},
		});
	};
	let actual = mutation_for(connection, &operation.account_id, operation.phase)?;
	if operation.target.as_ref() == Some(target) {
		return Ok(AccountLifecycleMutationOutcome::Replayed(actual));
	}
	if operation.phase != AccountOperationPhase::ProviderEffectPending
		|| operation.target.is_some()
		|| operation.superseded_by_operation_id.is_some()
		|| target.writer_operation_id != *operation_id
	{
		return Ok(AccountLifecycleMutationOutcome::Rejected {
			rejection: AccountLifecycleRejection::StaleOperation,
			actual,
		});
	}
	connection
		.execute(
			"UPDATE account_operations SET target_credential_json = ?1, updated_at_micros = ?2
			 WHERE operation_id = ?3 AND phase = 'provider_effect_pending'
			   AND target_credential_json IS NULL",
			params![
				binding_json(Some(target))?,
				unix_micros().map_err(StoreError::from)?,
				operation_id.as_str(),
			],
		)
		.map_err(sql_error)?;
	Ok(AccountLifecycleMutationOutcome::Applied(actual))
}

fn read_operation_sync(
	connection: &Connection,
	operation_id: &AccountOperationId,
) -> Result<Option<AccountOperation>, StoreError> {
	connection
		.query_row(
			"SELECT account_id, kind, phase, expected_account_revision,
			        requested_display_label, requested_enabled,
			        expected_credential_json, target_credential_json, recovery_code,
			        recovery_operation_id, superseded_by_operation_id
			 FROM account_operations WHERE operation_id = ?1",
			params![operation_id.as_str()],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, Option<i64>>(3)?,
					row.get::<_, Option<String>>(4)?,
					row.get::<_, Option<bool>>(5)?,
					row.get::<_, Option<String>>(6)?,
					row.get::<_, Option<String>>(7)?,
					row.get::<_, Option<String>>(8)?,
					row.get::<_, Option<String>>(9)?,
					row.get::<_, Option<String>>(10)?,
				))
			},
		)
		.optional()
		.map_err(sql_error)?
		.map(
			|(
				account_id,
				kind,
				phase,
				revision,
				label,
				enabled,
				expected,
				target,
				recovery_code,
				recovery_operation_id,
				superseded_by_operation_id,
			)| {
				Ok(AccountOperation {
					operation_id: operation_id.clone(),
					account_id: AccountId::new(account_id)
						.map_err(|_| incompatible("account identity"))?,
					kind: parse_operation_kind(&kind)?,
					phase: parse_operation_phase(&phase)?,
					expected_account_revision: revision,
					requested_display_label: label,
					requested_enabled: enabled,
					expected: parse_binding_json(expected.as_deref())?,
					target: parse_binding_json(target.as_deref())?,
					recovery_code,
					recovery_operation_id: recovery_operation_id
						.map(AccountOperationId::new)
						.transpose()
						.map_err(|_| incompatible("account recovery operation identity"))?,
					superseded_by_operation_id: superseded_by_operation_id
						.map(AccountOperationId::new)
						.transpose()
						.map_err(|_| incompatible("account superseding operation identity"))?,
				})
			},
		)
		.transpose()
}

pub(crate) fn read_account_registry_sync(
	connection: &Connection,
	account_id: Option<&str>,
	limit: u16,
) -> Result<Vec<AccountRecord>, StoreError> {
	if account_id.is_none() {
		let count: i64 = connection
			.query_row(
				"SELECT COUNT(*) FROM accounts WHERE tombstoned_at_micros IS NULL",
				[],
				|row| row.get(0),
			)
			.map_err(sql_error)?;
		if count > i64::from(limit) {
			return Err(StoreError::CapacityExhausted("account registry"));
		}
	}
	let capability_ready: bool = connection
		.query_row(
			"SELECT EXISTS (
			   SELECT 1 FROM codex_account_capability
			   WHERE singleton = 1 AND login_chatgpt_auth_tokens = 1 AND refresh_callback = 1
			 )",
			[],
			|row| row.get(0),
		)
		.map_err(sql_error)?;
	let mut statement = connection
		.prepare(
			"SELECT a.account_id, a.display_label, a.enabled, a.state, a.revision,
			        a.provider, a.provider_account_id, a.credential_store_observation,
			        a.tombstoned_at_micros IS NOT NULL
			 FROM accounts AS a
			 LEFT JOIN account_routing_order AS ordering USING (account_id)
			 WHERE (?1 IS NULL AND a.tombstoned_at_micros IS NULL) OR a.account_id = ?1
			 ORDER BY COALESCE(ordering.position, 2147483647), a.account_id LIMIT ?2",
		)
		.map_err(sql_error)?;
	let rows = statement
		.query_map(params![account_id, i64::from(limit)], |row| {
			Ok(AccountBase {
				account_id: row.get(0)?,
				label: row.get(1)?,
				enabled: row.get(2)?,
				state: row.get(3)?,
				revision: row.get(4)?,
				provider: row.get(5)?,
				provider_account_id: row.get(6)?,
				store_observation: row.get(7)?,
				tombstoned: row.get(8)?,
			})
		})
		.map_err(sql_error)?;
	let bases = rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)?;
	bases.into_iter().map(|base| account_from_base(connection, base, capability_ready)).collect()
}

fn account_from_base(
	connection: &Connection,
	base: AccountBase,
	capability_ready: bool,
) -> Result<AccountRecord, StoreError> {
	let account_id =
		AccountId::new(base.account_id).map_err(|_| incompatible("account identity"))?;
	let credential = credential_binding_sync(connection, &account_id)?;
	let unsettled_operation = unsettled_operation_sync(connection, &account_id)?;
	let provider_matches = credential.as_ref().is_none_or(|binding| {
		provider_text(binding.provider.provider()) == base.provider
			&& binding.provider.account_id() == base.provider_account_id
	});
	let lifecycle_readiness = if base.tombstoned {
		AccountLifecycleReadiness::Tombstoned
	} else if unsettled_operation.is_some() {
		AccountLifecycleReadiness::OperationUnsettled
	} else if credential.is_none() {
		AccountLifecycleReadiness::CredentialAbsent
	} else if !provider_matches {
		AccountLifecycleReadiness::ProviderMismatch
	} else if !capability_ready {
		AccountLifecycleReadiness::CallbackCapabilityUnready
	} else {
		match base.store_observation.as_str() {
			"exact" => AccountLifecycleReadiness::Ready,
			"unavailable" => AccountLifecycleReadiness::StoreUnavailable,
			"provider_mismatch" => AccountLifecycleReadiness::ProviderMismatch,
			"unknown" | "missing" | "mismatch" => AccountLifecycleReadiness::StoreMismatch,
			_ => return Err(incompatible("account store observation")),
		}
	};
	Ok(AccountRecord {
		account_id: account_id.clone(),
		label: base.label,
		enabled: base.enabled,
		revision: base.revision,
		observed_state: parse_account_state(&base.state)?,
		lifecycle_readiness,
		credential,
		unsettled_operation,
		five_hour_quota: quota_observation_sync(
			connection,
			&account_id,
			AccountQuotaWindow::FIVE_HOURS_MINUTES,
		)?,
		seven_day_quota: quota_observation_sync(
			connection,
			&account_id,
			AccountQuotaWindow::SEVEN_DAYS_MINUTES,
		)?,
		tombstoned: base.tombstoned,
	})
}

fn account_base_sync(
	connection: &Connection,
	account_id: &AccountId,
) -> Result<Option<AccountBase>, StoreError> {
	connection
		.query_row(
			"SELECT account_id, display_label, enabled, state, revision, provider,
			        provider_account_id, credential_store_observation,
			        tombstoned_at_micros IS NOT NULL
			 FROM accounts WHERE account_id = ?1",
			params![account_id.as_str()],
			|row| {
				Ok(AccountBase {
					account_id: row.get(0)?,
					label: row.get(1)?,
					enabled: row.get(2)?,
					state: row.get(3)?,
					revision: row.get(4)?,
					provider: row.get(5)?,
					provider_account_id: row.get(6)?,
					store_observation: row.get(7)?,
					tombstoned: row.get(8)?,
				})
			},
		)
		.optional()
		.map_err(sql_error)
}

fn credential_binding_sync(
	connection: &Connection,
	account_id: &AccountId,
) -> Result<Option<CredentialBinding>, StoreError> {
	connection
		.query_row(
			"SELECT schema_version, credential_version, fingerprint, writer_operation_id,
			        provider, provider_account_id
			 FROM account_credentials WHERE account_id = ?1",
			params![account_id.as_str()],
			|row| {
				Ok((
					row.get::<_, i64>(0)?,
					row.get::<_, i64>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, String>(3)?,
					row.get::<_, String>(4)?,
					row.get::<_, String>(5)?,
				))
			},
		)
		.optional()
		.map_err(sql_error)?
		.map(|(schema, version, fingerprint, writer, provider, provider_account)| {
			binding_from_parts(schema, version, fingerprint, writer, provider, provider_account)
		})
		.transpose()
}

fn unsettled_operation_sync(
	connection: &Connection,
	account_id: &AccountId,
) -> Result<Option<AccountOperationStatus>, StoreError> {
	connection
		.query_row(
			"SELECT operation_id, kind, phase, recovery_code FROM account_operations
			 WHERE account_id = ?1 AND phase NOT IN ('committed', 'cancelled')
			   AND superseded_by_operation_id IS NULL
			 ORDER BY recovery_operation_id IS NULL, created_at_micros, operation_id
			 LIMIT 1",
			params![account_id.as_str()],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, Option<String>>(3)?,
				))
			},
		)
		.optional()
		.map_err(sql_error)?
		.map(|(id, kind, phase, recovery_code)| {
			Ok(AccountOperationStatus {
				operation_id: AccountOperationId::new(id)
					.map_err(|_| incompatible("account operation identity"))?,
				kind: parse_operation_kind(&kind)?,
				phase: parse_operation_phase(&phase)?,
				recovery_code,
			})
		})
		.transpose()
}

fn quota_observation_sync(
	connection: &Connection,
	account_id: &AccountId,
	duration: u32,
) -> Result<AccountQuotaWindowObservation, StoreError> {
	let row = connection
		.query_row(
			"SELECT used_percent, resets_at_micros, error_code, observed_at_micros
			 FROM account_quota_facts WHERE account_id = ?1 AND duration_minutes = ?2",
			params![account_id.as_str(), i64::from(duration)],
			|row| {
				Ok((
					row.get::<_, Option<i64>>(0)?,
					row.get::<_, Option<i64>>(1)?,
					row.get::<_, Option<String>>(2)?,
					row.get::<_, i64>(3)?,
				))
			},
		)
		.optional()
		.map_err(sql_error)?;
	let Some((used, resets, error, observed)) = row else {
		return AccountQuotaWindowObservation::unknown(duration)
			.map_err(|_| incompatible("quota duration"));
	};
	if error.as_deref() == Some("unsupported_window") {
		return AccountQuotaWindowObservation::unknown(duration)
			.map_err(|_| incompatible("quota duration"));
	}
	let disposition = match (used, resets, error.as_deref()) {
		(None, None, Some(error)) => AccountQuotaDisposition::Error(parse_quota_error(error)?),
		(Some(used), Some(resets), None) => {
			let fact = AccountQuotaWindow::new(
				duration,
				u8::try_from(used).map_err(|_| incompatible("quota percentage"))?,
				resets,
			)
			.map_err(|_| incompatible("quota window"))?;
			let now = unix_micros().map_err(StoreError::from)?;
			if resets <= now || observed.saturating_add(QUOTA_FRESHNESS_MICROS) < now {
				AccountQuotaDisposition::Stale(fact)
			} else {
				AccountQuotaDisposition::Current(fact)
			}
		},
		_ => return Err(incompatible("quota observation shape")),
	};
	Ok(AccountQuotaWindowObservation {
		duration_minutes: duration,
		observed_at_unix_micros: Some(observed),
		disposition,
	})
}

fn read_routing_control_sync(connection: &Connection) -> Result<AccountRoutingControl, StoreError> {
	let (mode, fixed, revision): (String, Option<String>, i64) = connection
		.query_row(
			"SELECT mode, fixed_account_id, revision FROM account_routing_control WHERE singleton = 1",
			[],
			|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
		)
		.map_err(sql_error)?;
	let mut statement = connection
		.prepare(
			"SELECT ordering.account_id FROM account_routing_order AS ordering
			 JOIN accounts AS account USING (account_id)
			 WHERE account.tombstoned_at_micros IS NULL
			 ORDER BY ordering.position, ordering.account_id",
		)
		.map_err(sql_error)?;
	let rows = statement.query_map([], |row| row.get::<_, String>(0)).map_err(sql_error)?;
	let order = rows
		.collect::<Result<Vec<_>, _>>()
		.map_err(sql_error)?
		.into_iter()
		.map(|id| AccountId::new(id).map_err(|_| incompatible("account routing identity")))
		.collect::<Result<Vec<_>, _>>()?;
	let mode = match mode.as_str() {
		"balanced" if fixed.is_none() => AccountSelectionMode::Balanced,
		"fixed" => AccountSelectionMode::Fixed(
			AccountId::new(fixed.ok_or_else(|| incompatible("fixed account identity"))?)
				.map_err(|_| incompatible("fixed account identity"))?,
		),
		_ => return Err(incompatible("account selection mode")),
	};
	Ok(AccountRoutingControl { revision, mode, order })
}

fn set_account_enabled_sync(
	connection: &Connection,
	account_id: &AccountId,
	expected_revision: i64,
	enabled: bool,
) -> Result<AccountAdministrationOutcome, StoreError> {
	let Some(account) = account_base_sync(connection, account_id)? else {
		return Ok(AccountAdministrationOutcome::Rejected {
			rejection: AccountLifecycleRejection::AccountMissing,
			revision: 0,
		});
	};
	if account.tombstoned {
		return Ok(AccountAdministrationOutcome::Rejected {
			rejection: AccountLifecycleRejection::AccountMissing,
			revision: account.revision,
		});
	}
	if account.revision != expected_revision {
		return Ok(AccountAdministrationOutcome::Rejected {
			rejection: AccountLifecycleRejection::StaleAccount,
			revision: account.revision,
		});
	}
	let revision = expected_revision
		.checked_add(1)
		.ok_or(StoreError::CapacityExhausted("account revision"))?;
	connection
		.execute(
			"UPDATE accounts SET enabled = ?1, revision = ?2, updated_at_micros = ?3
			 WHERE account_id = ?4 AND revision = ?5 AND tombstoned_at_micros IS NULL",
			params![
				enabled,
				revision,
				unix_micros().map_err(StoreError::from)?,
				account_id.as_str(),
				expected_revision,
			],
		)
		.map_err(sql_error)?;
	Ok(AccountAdministrationOutcome::Updated { revision })
}

fn set_fixed_routing_sync(
	connection: &Connection,
	expected_routing_revision: i64,
	account_id: &AccountId,
	expected_account_revision: i64,
) -> Result<RoutingControlOutcome, StoreError> {
	let routing = read_routing_control_sync(connection)?;
	if routing.revision != expected_routing_revision {
		return Ok(RoutingControlOutcome::StaleRoutingControl { revision: routing.revision });
	}
	let Some(account) = account_base_sync(connection, account_id)? else {
		return Ok(RoutingControlOutcome::AccountMissing);
	};
	if account.tombstoned {
		return Ok(RoutingControlOutcome::AccountMissing);
	}
	if account.revision != expected_account_revision {
		return Ok(RoutingControlOutcome::StaleAccount { revision: account.revision });
	}
	let revision =
		routing.revision.checked_add(1).ok_or(StoreError::CapacityExhausted("routing revision"))?;
	connection
		.execute(
			"UPDATE account_routing_control SET mode = 'fixed', fixed_account_id = ?1,
			 revision = ?2, updated_at_micros = ?3 WHERE singleton = 1 AND revision = ?4",
			params![
				account_id.as_str(),
				revision,
				unix_micros().map_err(StoreError::from)?,
				expected_routing_revision,
			],
		)
		.map_err(sql_error)?;
	Ok(RoutingControlOutcome::Updated { routing: read_routing_control_sync(connection)? })
}

fn set_balanced_routing_sync(
	connection: &Connection,
	expected_routing_revision: i64,
) -> Result<RoutingControlOutcome, StoreError> {
	let routing = read_routing_control_sync(connection)?;
	if routing.revision != expected_routing_revision {
		return Ok(RoutingControlOutcome::StaleRoutingControl { revision: routing.revision });
	}
	let revision =
		routing.revision.checked_add(1).ok_or(StoreError::CapacityExhausted("routing revision"))?;
	connection
		.execute(
			"UPDATE account_routing_control SET mode = 'balanced', fixed_account_id = NULL,
			 revision = ?1, updated_at_micros = ?2 WHERE singleton = 1 AND revision = ?3",
			params![revision, unix_micros().map_err(StoreError::from)?, expected_routing_revision,],
		)
		.map_err(sql_error)?;
	Ok(RoutingControlOutcome::Updated { routing: read_routing_control_sync(connection)? })
}

fn set_account_order_sync(
	connection: &Connection,
	expected_routing_revision: i64,
	order: &[AccountId],
) -> Result<RoutingControlOutcome, StoreError> {
	let routing = read_routing_control_sync(connection)?;
	if routing.revision != expected_routing_revision {
		return Ok(RoutingControlOutcome::StaleRoutingControl { revision: routing.revision });
	}
	let visible = routing.order.iter().cloned().collect::<BTreeSet<_>>();
	let proposed = order.iter().cloned().collect::<BTreeSet<_>>();
	if visible != proposed || proposed.len() != order.len() {
		return Ok(RoutingControlOutcome::InvalidOrder { revision: routing.revision });
	}
	let now = unix_micros().map_err(StoreError::from)?;
	connection.execute("DELETE FROM account_routing_order", []).map_err(sql_error)?;
	for (position, account_id) in order.iter().enumerate() {
		connection
			.execute(
				"INSERT INTO account_routing_order (account_id, position, updated_at_micros)
				 VALUES (?1, ?2, ?3)",
				params![
					account_id.as_str(),
					i64::try_from(position)
						.map_err(|_| StoreError::CapacityExhausted("account routing order"))?,
					now,
				],
			)
			.map_err(sql_error)?;
	}
	bump_routing_revision(connection, now)?;
	Ok(RoutingControlOutcome::Updated { routing: read_routing_control_sync(connection)? })
}

fn compact_routing_order(connection: &Connection, now: i64) -> Result<(), StoreError> {
	let mut statement = connection
		.prepare("SELECT account_id FROM account_routing_order ORDER BY position, account_id")
		.map_err(sql_error)?;
	let rows = statement.query_map([], |row| row.get::<_, String>(0)).map_err(sql_error)?;
	let ids = rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)?;
	drop(statement);
	connection
		.execute(
			"UPDATE account_routing_order SET position = position + ?1, updated_at_micros = ?2",
			params![
				i64::try_from(MAX_ACCOUNT_COUNT)
					.map_err(|_| StoreError::CapacityExhausted("account routing order"))?,
				now,
			],
		)
		.map_err(sql_error)?;
	for (position, account_id) in ids.iter().enumerate() {
		connection
			.execute(
				"UPDATE account_routing_order SET position = ?1, updated_at_micros = ?2
				 WHERE account_id = ?3",
				params![
					i64::try_from(position)
						.map_err(|_| StoreError::CapacityExhausted("account routing order"))?,
					now,
					account_id,
				],
			)
			.map_err(sql_error)?;
	}
	Ok(())
}

fn bump_routing_revision(connection: &Connection, now: i64) -> Result<(), StoreError> {
	connection
		.execute(
			"UPDATE account_routing_control
			 SET revision = revision + 1, updated_at_micros = ?1 WHERE singleton = 1",
			params![now],
		)
		.map_err(sql_error)?;
	Ok(())
}

fn mutation_for(
	connection: &Connection,
	account_id: &AccountId,
	phase: AccountOperationPhase,
) -> Result<AccountLifecycleMutation, StoreError> {
	Ok(AccountLifecycleMutation {
		account_revision: account_base_sync(connection, account_id)?
			.map_or(0, |value| value.revision),
		phase,
	})
}

fn account_revision_sync(
	connection: &Connection,
	account_id: &AccountId,
) -> Result<i64, StoreError> {
	account_base_sync(connection, account_id)?
		.map(|value| value.revision)
		.ok_or(StoreError::InvalidInput("account is absent"))
}

fn binding_json(binding: Option<&CredentialBinding>) -> Result<Option<String>, StoreError> {
	binding
		.map(|binding| {
			serde_json::to_string(&json!({
				"schema_version": binding.schema_version.get(),
				"credential_version": binding.version.get(),
				"fingerprint": binding.fingerprint.as_str(),
				"writer_operation_id": binding.writer_operation_id.as_str(),
				"provider": provider_text(binding.provider.provider()),
				"provider_account_id": binding.provider.account_id(),
			}))
			.map_err(|_| StoreError::InvalidInput("credential binding is invalid"))
		})
		.transpose()
}

fn parse_binding_json(value: Option<&str>) -> Result<Option<CredentialBinding>, StoreError> {
	let Some(value) = value else { return Ok(None) };
	let value: Value =
		serde_json::from_str(value).map_err(|_| incompatible("credential binding"))?;
	let schema = value
		.get("schema_version")
		.and_then(Value::as_u64)
		.and_then(|value| i64::try_from(value).ok())
		.ok_or_else(|| incompatible("credential binding schema"))?;
	let version = value
		.get("credential_version")
		.and_then(Value::as_u64)
		.and_then(|value| i64::try_from(value).ok())
		.ok_or_else(|| incompatible("credential binding version"))?;
	binding_from_parts(
		schema,
		version,
		json_string(&value, "fingerprint")?,
		json_string(&value, "writer_operation_id")?,
		json_string(&value, "provider")?,
		json_string(&value, "provider_account_id")?,
	)
	.map(Some)
}

fn json_string(value: &Value, key: &str) -> Result<String, StoreError> {
	value
		.get(key)
		.and_then(Value::as_str)
		.map(str::to_owned)
		.ok_or_else(|| incompatible("credential binding field"))
}

fn binding_from_parts(
	schema: i64,
	version: i64,
	fingerprint: String,
	writer: String,
	provider: String,
	provider_account: String,
) -> Result<CredentialBinding, StoreError> {
	let provider = match provider.as_str() {
		"chatgpt" => AccountProvider::Chatgpt,
		_ => return Err(incompatible("credential provider")),
	};
	Ok(CredentialBinding {
		schema_version: CredentialStoreSchemaVersion::new(
			u16::try_from(schema).map_err(|_| incompatible("credential schema"))?,
		)
		.map_err(|_| incompatible("credential schema"))?,
		version: CredentialVersion::new(
			u64::try_from(version).map_err(|_| incompatible("credential version"))?,
		)
		.map_err(|_| incompatible("credential version"))?,
		fingerprint: CredentialFingerprint::new(fingerprint)
			.map_err(|_| incompatible("credential fingerprint"))?,
		provider: ProviderIdentity::new(provider, provider_account)
			.map_err(|_| incompatible("credential provider identity"))?,
		writer_operation_id: AccountOperationId::new(writer)
			.map_err(|_| incompatible("credential writer operation identity"))?,
	})
}

fn validate_preparation(preparation: &AccountOperationPreparation) -> Result<(), StoreError> {
	if preparation.expected_account_revision.is_some_and(|revision| revision < 1) {
		return Err(StoreError::InvalidInput("expected account revision must be positive"));
	}
	let new =
		matches!(preparation.kind, AccountOperationKind::Enroll | AccountOperationKind::Import);
	if new
		!= (preparation.display_label.is_some()
			&& preparation.enabled.is_some()
			&& preparation.expected_account_revision.is_none()
			&& preparation.expected.is_none()
			&& preparation.target.is_some())
	{
		return Err(StoreError::InvalidInput("account operation shape is invalid"));
	}
	if !new
		&& (preparation.display_label.is_some()
			|| preparation.enabled.is_some()
			|| preparation.expected_account_revision.is_none()
			|| preparation.expected.is_none())
	{
		return Err(StoreError::InvalidInput("account operation shape is invalid"));
	}
	if preparation.display_label.as_ref().is_some_and(|label| {
		label.is_empty()
			|| label.len() > 128
			|| label.chars().any(char::is_control)
			|| decodex_core::contains_credential_material(label)
	}) {
		return Err(StoreError::InvalidInput("account label is invalid"));
	}
	if preparation.expected.as_ref().is_some_and(|binding| binding.provider != preparation.provider)
		|| preparation.target.as_ref().is_some_and(|binding| {
			binding.provider != preparation.provider
				|| binding.writer_operation_id != preparation.operation_id
		}) {
		return Err(StoreError::InvalidInput("credential binding is invalid"));
	}
	Ok(())
}

fn validate_limit(limit: u16, reason: &'static str) -> Result<(), StoreError> {
	validate_bounded_limit(limit, MAX_ACCOUNT_COUNT, reason)
}

fn validate_bounded_limit(
	limit: u16,
	maximum: usize,
	reason: &'static str,
) -> Result<(), StoreError> {
	if limit == 0 || usize::from(limit) > maximum {
		Err(StoreError::InvalidInput(reason))
	} else {
		Ok(())
	}
}

fn validate_recovery_code(code: Option<&str>) -> Result<(), StoreError> {
	if code.is_some_and(|code| {
		code.is_empty()
			|| code.len() > 128
			|| !code.bytes().enumerate().all(|(index, byte)| {
				if index == 0 {
					byte.is_ascii_lowercase()
				} else {
					byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
				}
			})
	}) {
		Err(StoreError::InvalidInput("account recovery code is invalid"))
	} else {
		Ok(())
	}
}

fn validate_transition_recovery_shape(
	target: AccountOperationPhase,
	code: Option<&str>,
) -> Result<(), StoreError> {
	if (target == AccountOperationPhase::RecoveryRequired) != code.is_some() {
		Err(StoreError::InvalidInput("account recovery transition shape is invalid"))
	} else {
		Ok(())
	}
}

fn validate_account_revision(revision: i64) -> Result<(), StoreError> {
	if revision < 1 {
		Err(StoreError::InvalidInput("expected account revision must be positive"))
	} else {
		Ok(())
	}
}

fn validate_routing_revision(revision: i64) -> Result<(), StoreError> {
	if revision < 1 {
		Err(StoreError::InvalidInput("expected routing revision must be positive"))
	} else {
		Ok(())
	}
}

fn validate_order(order: &[AccountId]) -> Result<(), StoreError> {
	if order.len() > MAX_ACCOUNT_COUNT
		|| order.iter().cloned().collect::<BTreeSet<_>>().len() != order.len()
	{
		Err(StoreError::InvalidInput("account routing order is invalid"))
	} else {
		Ok(())
	}
}

fn validate_registry_snapshot(
	accounts: &[AccountRecord],
	routing: &AccountRoutingControl,
) -> Result<(), StoreError> {
	let universe =
		accounts.iter().map(|account| account.account_id.clone()).collect::<BTreeSet<_>>();
	let ordered = routing.order.iter().cloned().collect::<BTreeSet<_>>();
	if universe.len() != accounts.len()
		|| ordered.len() != routing.order.len()
		|| universe != ordered
		|| accounts.iter().any(|account| account.tombstoned)
	{
		return Err(incompatible("account registry routing universe"));
	}
	if let AccountSelectionMode::Fixed(account_id) = &routing.mode
		&& !universe.contains(account_id)
	{
		return Err(incompatible("fixed account routing target"));
	}
	Ok(())
}

fn allowed_operation_transition(
	expected: AccountOperationPhase,
	target: AccountOperationPhase,
) -> bool {
	matches!(
		(expected, target),
		(AccountOperationPhase::Prepared, AccountOperationPhase::ProviderEffectPending)
			| (AccountOperationPhase::Prepared, AccountOperationPhase::StoreApplied)
			| (AccountOperationPhase::Prepared, AccountOperationPhase::Cancelled)
			| (AccountOperationPhase::Prepared, AccountOperationPhase::RecoveryRequired)
			| (AccountOperationPhase::ProviderEffectPending, AccountOperationPhase::StoreApplied)
			| (AccountOperationPhase::ProviderEffectPending, AccountOperationPhase::Cancelled)
			| (
				AccountOperationPhase::ProviderEffectPending,
				AccountOperationPhase::RecoveryRequired
			) | (AccountOperationPhase::StoreApplied, AccountOperationPhase::Committed)
			| (AccountOperationPhase::StoreApplied, AccountOperationPhase::RecoveryRequired)
			| (AccountOperationPhase::RecoveryRequired, AccountOperationPhase::StoreApplied)
			| (AccountOperationPhase::RecoveryRequired, AccountOperationPhase::Cancelled)
	)
}

pub(crate) fn random_uuid_v4() -> Result<String, StoreError> {
	let mut bytes = [0_u8; 16];
	getrandom::fill(&mut bytes).map_err(|_| StoreError::Database(DatabaseError::Unavailable))?;
	bytes[6] = (bytes[6] & 0x0f) | 0x40;
	bytes[8] = (bytes[8] & 0x3f) | 0x80;
	Ok(format!(
		"{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
		bytes[0],
		bytes[1],
		bytes[2],
		bytes[3],
		bytes[4],
		bytes[5],
		bytes[6],
		bytes[7],
		bytes[8],
		bytes[9],
		bytes[10],
		bytes[11],
		bytes[12],
		bytes[13],
		bytes[14],
		bytes[15],
	))
}

fn validate_account_command_response(value: &Value) -> Result<(), StoreError> {
	let bytes = serde_json::to_vec(value)
		.map_err(|_| StoreError::InvalidInput("account command result is invalid"))?;
	if bytes.len() > 256 * 1024 {
		return Err(StoreError::InvalidInput("account command result is invalid"));
	}
	ensure_credential_negative_json(value)
}

fn ensure_credential_negative_json(value: &Value) -> Result<(), StoreError> {
	match value {
		Value::Object(entries) =>
			for (key, value) in entries {
				if decodex_core::is_credential_metadata_key(key) {
					return Err(StoreError::CredentialRejected);
				}
				ensure_credential_negative_json(value)?;
			},
		Value::Array(entries) =>
			for value in entries {
				ensure_credential_negative_json(value)?;
			},
		Value::String(value) if decodex_core::contains_credential_material(value) => {
			return Err(StoreError::CredentialRejected);
		},
		_ => {},
	}
	Ok(())
}

const fn operation_kind_text(value: AccountOperationKind) -> &'static str {
	match value {
		AccountOperationKind::Enroll => "enroll",
		AccountOperationKind::Import => "import",
		AccountOperationKind::Refresh => "refresh",
		AccountOperationKind::Logout => "logout",
	}
}

fn parse_operation_kind(value: &str) -> Result<AccountOperationKind, StoreError> {
	match value {
		"enroll" => Ok(AccountOperationKind::Enroll),
		"import" => Ok(AccountOperationKind::Import),
		"refresh" => Ok(AccountOperationKind::Refresh),
		"logout" => Ok(AccountOperationKind::Logout),
		_ => Err(incompatible("account operation kind")),
	}
}

const fn operation_phase_text(value: AccountOperationPhase) -> &'static str {
	match value {
		AccountOperationPhase::Prepared => "prepared",
		AccountOperationPhase::ProviderEffectPending => "provider_effect_pending",
		AccountOperationPhase::StoreApplied => "store_applied",
		AccountOperationPhase::Committed => "committed",
		AccountOperationPhase::Cancelled => "cancelled",
		AccountOperationPhase::RecoveryRequired => "recovery_required",
	}
}

fn parse_operation_phase(value: &str) -> Result<AccountOperationPhase, StoreError> {
	match value {
		"prepared" => Ok(AccountOperationPhase::Prepared),
		"provider_effect_pending" => Ok(AccountOperationPhase::ProviderEffectPending),
		"store_applied" => Ok(AccountOperationPhase::StoreApplied),
		"committed" => Ok(AccountOperationPhase::Committed),
		"cancelled" => Ok(AccountOperationPhase::Cancelled),
		"recovery_required" => Ok(AccountOperationPhase::RecoveryRequired),
		_ => Err(incompatible("account operation phase")),
	}
}

pub(crate) fn parse_account_state(value: &str) -> Result<AccountState, StoreError> {
	match value {
		"unavailable" => Ok(AccountState::Unavailable),
		"unknown" => Ok(AccountState::Unknown),
		"available" => Ok(AccountState::Available),
		"depleted" => Ok(AccountState::Depleted),
		"auth_failed" => Ok(AccountState::AuthFailed),
		"plugin_unready" => Ok(AccountState::PluginUnready),
		_ => Err(incompatible("account state")),
	}
}

const fn provider_text(provider: AccountProvider) -> &'static str {
	match provider {
		AccountProvider::Chatgpt => "chatgpt",
	}
}

const fn store_observation_text(observation: AccountStoreObservation) -> &'static str {
	match observation {
		AccountStoreObservation::Exact => "exact",
		AccountStoreObservation::Missing => "missing",
		AccountStoreObservation::Mismatch => "mismatch",
		AccountStoreObservation::ProviderMismatch => "provider_mismatch",
		AccountStoreObservation::Unavailable => "unavailable",
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

fn parse_quota_error(value: &str) -> Result<AccountQuotaObservationError, StoreError> {
	match value {
		"provider_unavailable" => Ok(AccountQuotaObservationError::ProviderUnavailable),
		"protocol_unavailable" => Ok(AccountQuotaObservationError::ProtocolUnavailable),
		"account_mismatch" => Ok(AccountQuotaObservationError::AccountMismatch),
		"unsupported_window" => Ok(AccountQuotaObservationError::UnsupportedWindow),
		_ => Err(incompatible("quota observation error")),
	}
}

fn is_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn sql_error(_error: rusqlite::Error) -> StoreError {
	StoreError::Database(DatabaseError::Unavailable)
}

fn incompatible(value: &'static str) -> StoreError {
	StoreError::Incompatible(format!("stored {value} is malformed"))
}
