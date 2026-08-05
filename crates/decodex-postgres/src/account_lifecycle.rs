//! Credential-negative PostgreSQL account lifecycle authority.

use std::{
	collections::BTreeSet,
	fmt::{Display, Formatter},
};

#[cfg(unix)] use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use decodex_core::{
	AccountId, AccountLifecycleReadiness, AccountOperation, AccountOperationId,
	AccountOperationKind, AccountOperationPhase, AccountOperationStatus, AccountProvider,
	AccountQuotaDisposition, AccountQuotaObservationError, AccountQuotaWindow,
	AccountQuotaWindowObservation, AccountRecord, AccountRoutingControl, AccountSelectionMode,
	AccountState, CredentialBinding, CredentialFingerprint, CredentialStoreSchemaVersion,
	CredentialVersion, PostgresConnectionConfig, PostgresIdentityConfig, ProviderIdentity,
};
use serde_json::Value;
use tokio_postgres::IsolationLevel;
#[cfg(unix)] use tokio_postgres::{GenericClient, Transaction};

use crate::{
	BootstrapFailure, CommandIdentity, PostgresStore, StoreError,
	accounts::{
		CommandClaim, CommandDescriptor, CommandReservation, finish_command, reserve_command,
	},
};
#[cfg(unix)]
use crate::{
	apply_trusted_session_invariants, authority, checkout, connection_config, schema,
	validate_connection, verified_socket_connect,
};

const ACCOUNT_COMMAND_PROTOCOL: &str = "decodex/account-command/1";
const READ_ACCOUNT_REGISTRY_ALL_SQL: &str = "SELECT account_id::text,display_label,enabled,state::text,revision,\
	 provider_kind::text,provider_account_id,credential_store_schema_version,\
	 credential_version,credential_fingerprint,credential_writer_operation_id::text,\
	 tombstoned,lifecycle_readiness,unsettled_operation_id::text,\
	 unsettled_kind::text,unsettled_phase::text,unsettled_recovery_code,\
	 five_hour_disposition,five_hour_used_percent,five_hour_resets_at_micros,\
	 five_hour_observed_at_micros,five_hour_error_code::text,\
	 seven_day_disposition,seven_day_used_percent,seven_day_resets_at_micros,\
	 seven_day_observed_at_micros,seven_day_error_code::text \
	 FROM decodex.read_account_registry_exact(NULL,$1)";
const READ_ACCOUNT_REGISTRY_SQL: &str = "SELECT account_id::text,display_label,enabled,state::text,revision,\
	 provider_kind::text,provider_account_id,credential_store_schema_version,\
	 credential_version,credential_fingerprint,credential_writer_operation_id::text,\
	 tombstoned,lifecycle_readiness,unsettled_operation_id::text,\
	 unsettled_kind::text,unsettled_phase::text,unsettled_recovery_code,\
	 five_hour_disposition,five_hour_used_percent,five_hour_resets_at_micros,\
	 five_hour_observed_at_micros,five_hour_error_code::text,\
	 seven_day_disposition,seven_day_used_percent,seven_day_resets_at_micros,\
	 seven_day_observed_at_micros,seven_day_error_code::text \
	 FROM decodex.read_account_registry_exact($1::text::uuid,$2)";
const READ_ACCOUNT_EXACT_SQL: &str = "SELECT account_id::text,display_label,enabled,state::text,revision,\
	 provider_kind::text,provider_account_id,credential_store_schema_version,\
	 credential_version,credential_fingerprint,credential_writer_operation_id::text,\
	 tombstoned,lifecycle_readiness,unsettled_operation_id::text,\
	 unsettled_kind::text,unsettled_phase::text,unsettled_recovery_code,\
	 five_hour_disposition,five_hour_used_percent,five_hour_resets_at_micros,\
	 five_hour_observed_at_micros,five_hour_error_code::text,\
	 seven_day_disposition,seven_day_used_percent,seven_day_resets_at_micros,\
	 seven_day_observed_at_micros,seven_day_error_code::text \
	 FROM decodex.read_account_registry_exact($1::text::uuid,1)";
const READ_ACCOUNT_ROUTING_SQL: &str = "SELECT mode::text,fixed_account_id::text,revision,\
	 ARRAY(SELECT value::text FROM pg_catalog.unnest(account_order) AS value) \
	 FROM decodex.read_account_routing_control_exact()";
const PREPARE_ACCOUNT_OPERATION_SQL: &str = "SELECT result_code,account_revision,phase::text \
	 FROM decodex.prepare_account_operation_exact(\
	 $1::text::uuid,$2::text::uuid,$3::text::decodex.account_operation_kind,\
	 $4,$5,$6,$7,$8,$9,$10::text::uuid,$11,$12,$13,$14::text::uuid,\
	 $15::text::decodex.account_provider_kind,$16)";
const ADVANCE_ACCOUNT_OPERATION_SQL: &str = "SELECT result_code,account_revision,phase::text \
	 FROM decodex.advance_account_operation_exact(\
	 $1::text::uuid,$2::text::decodex.account_operation_phase,\
	 $3::text::decodex.account_operation_phase,$4)";
const SET_ACCOUNT_OPERATION_TARGET_SQL: &str = "SELECT result_code,account_revision,phase::text \
	 FROM decodex.set_account_operation_target_exact($1::text::uuid,$2,$3,$4,$5::text::uuid)";
const READ_UNSETTLED_ACCOUNT_OPERATIONS_SQL: &str = "SELECT operation_id::text,account_id::text,kind::text,phase::text,\
		 expected_account_revision,requested_display_label,requested_enabled,\
		 expected_store_schema_version,expected_credential_version,\
	 expected_credential_fingerprint,expected_credential_writer_operation_id::text,\
	 target_store_schema_version,target_credential_version,target_credential_fingerprint,\
	 target_credential_writer_operation_id::text,provider_kind::text,provider_account_id \
	 FROM decodex.read_unsettled_account_operations_exact($1)";
const READ_ACCOUNT_OPERATION_SQL: &str = "SELECT operation_id::text,account_id::text,kind::text,phase::text,\
		 expected_account_revision,requested_display_label,requested_enabled,\
		 expected_store_schema_version,expected_credential_version,\
	 expected_credential_fingerprint,expected_credential_writer_operation_id::text,\
	 target_store_schema_version,target_credential_version,target_credential_fingerprint,\
	 target_credential_writer_operation_id::text,provider_kind::text,provider_account_id \
	 FROM decodex.read_account_operation_exact($1::text::uuid)";
const SET_ACCOUNT_ENABLED_SQL: &str = "SELECT result_code,revision \
	 FROM decodex.set_account_enabled_exact($1::text::uuid,$2,$3)";
const SET_FIXED_ACCOUNT_SELECTION_SQL: &str = "SELECT result_code,routing_revision,account_revision \
	 FROM decodex.set_fixed_account_selection_exact($1,$2::text::uuid,$3)";
const SET_BALANCED_ACCOUNT_SELECTION_SQL: &str = "SELECT result_code,routing_revision \
	 FROM decodex.set_balanced_account_selection_exact($1)";
const SET_ACCOUNT_ORDER_SQL: &str = "SELECT result_code,routing_revision \
	 FROM decodex.set_account_order_exact($1,$2::text[]::uuid[])";
const OBSERVE_ACCOUNT_QUOTA_SQL: &str =
	"SELECT decodex.observe_account_quota_exact($1::text::uuid,$2,$3,$4,$5)";
const OBSERVE_ACCOUNT_QUOTA_ERROR_SQL: &str = "SELECT decodex.observe_account_quota_error_exact(\
	 $1::text::uuid,$2,$3::text::decodex.account_quota_observation_error,$4)";
const OBSERVE_ACCOUNT_STORE_SQL: &str = "SELECT decodex.observe_account_store_exact(\
	 $1::text::uuid,$2,$3,$4,$5,$6::text::uuid,\
	 $7::text::decodex.account_provider_kind,$8,\
	 $9::text::decodex.account_store_observation)";
const ATTEST_CODEX_ACCOUNT_CAPABILITY_SQL: &str =
	"SELECT decodex.attest_codex_account_capability_exact($1,$2,$3,$4,$5,$6)";
const RESTORE_LOCAL_ACCOUNT_SQL: &str = "INSERT INTO decodex.accounts(\
	 account_id,display_label,state,metadata,revision,enabled,provider_kind,provider_account_id,\
	 credential_store_schema_version,credential_version,credential_fingerprint,\
	 credential_writer_operation_id,credential_store_observation,credential_store_observed_at\
	 ) VALUES (\
	 $1::text::uuid,$2,'unknown','{}'::jsonb,$3,$4,$5::text::decodex.account_provider_kind,$6,\
	 $7,$8,$9,$10::text::uuid,'exact',pg_catalog.clock_timestamp())";
const READ_RESTORED_LOCAL_ACCOUNTS_SQL: &str = "SELECT account.account_id::text,\
	 account.display_label,account.enabled,account.revision,account.provider_kind::text,\
	 account.provider_account_id,account.credential_store_schema_version,\
	 account.credential_version,account.credential_fingerprint,\
	 account.credential_writer_operation_id::text,account.credential_store_observation::text,\
	 account.credential_store_observed_at IS NOT NULL,account.tombstoned_at IS NOT NULL,\
	 account.state::text,account.metadata FROM decodex.accounts AS account \
	 JOIN decodex.account_routing_order AS ordering USING(account_id) ORDER BY ordering.position";
const LOCAL_ACCOUNT_AUTHORITY_MAX_ACCOUNTS: usize = 512;

/// One credential-negative account row accepted by the bounded local restore transaction.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAccountAuthorityAccount {
	pub account_id: AccountId,
	pub display_label: String,
	pub enabled: bool,
	pub revision: i64,
	pub credential: CredentialBinding,
}

/// Complete account and routing tuple accepted by the bounded local restore transaction.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAccountAuthorityRestore {
	pub accounts: Vec<LocalAccountAuthorityAccount>,
	pub routing: AccountRoutingControl,
}

/// Credential-negative refusal from the schema-owner local account restore transaction.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalAccountAuthorityRestoreFailure {
	/// The internal credential-negative tuple was not complete and canonical.
	InvalidInput,
	/// The target contains state beyond a fresh latest-schema bootstrap.
	TargetNotFresh,
	/// The runtime-owned pre-commit host-store or stopped-daemon fence failed.
	PrecommitFence,
	/// PostgreSQL did not read back the complete accepted tuple exactly.
	ReadbackMismatch,
	/// Connection, catalog, or configured authority failed closed.
	Database(BootstrapFailure),
}
impl Display for LocalAccountAuthorityRestoreFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidInput => "local account authority input refused",
			Self::TargetNotFresh => "local account authority target is not fresh",
			Self::PrecommitFence => "local account authority pre-commit fence refused",
			Self::ReadbackMismatch => "local account authority readback refused",
			Self::Database(BootstrapFailure::Authentication) =>
				"local account authority authentication refused",
			Self::Database(BootstrapFailure::Unreachable) =>
				"local account authority database is unreachable",
			Self::Database(BootstrapFailure::Incompatible) =>
				"local account authority database is incompatible",
			Self::Database(BootstrapFailure::UnsafeAuthority) =>
				"local account authority database authority is unsafe",
			Self::Database(BootstrapFailure::UnsafeHostPath) =>
				"local account authority database path is unsafe",
		})
	}
}
impl std::error::Error for LocalAccountAuthorityRestoreFailure {}

/// Exact input persisted before the Account Service performs a Keychain effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountOperationPreparation {
	/// Stable finite operation identity.
	pub operation_id: AccountOperationId,
	/// Account changed by the operation.
	pub account_id: AccountId,
	/// Operation effect class.
	pub kind: AccountOperationKind,
	/// New-account operator label, when required.
	pub display_label: Option<String>,
	/// New-account administrative switch, when required.
	pub enabled: Option<bool>,
	/// Exact registry revision before an existing-account effect.
	pub expected_account_revision: Option<i64>,
	/// Exact credential binding before the effect.
	pub expected: Option<CredentialBinding>,
	/// Exact credential binding after the effect, when known.
	pub target: Option<CredentialBinding>,
	/// Non-secret provider identity for the operation.
	pub provider: ProviderIdentity,
}

/// Credential-negative operation mutation projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountLifecycleMutation {
	/// Account revision observed after the transition.
	pub account_revision: i64,
	/// Resulting finite operation phase.
	pub phase: AccountOperationPhase,
}

/// Closed rejection from account lifecycle persistence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountLifecycleRejection {
	/// The stable operation identity already has another descriptor.
	IdentityConflict,
	/// Another operation for the account is unsettled.
	OperationUnsettled,
	/// The operation shape or bounded input is invalid.
	InvalidRequest,
	/// The requested account does not exist.
	AccountMissing,
	/// The expected account revision or credential binding is stale.
	StaleAccount,
	/// Existing work prevents logout.
	AccountInUse,
	/// The requested operation does not exist.
	OperationMissing,
	/// The expected operation phase is stale.
	StaleOperation,
}

/// Result of preparing or advancing one finite operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountLifecycleMutationOutcome {
	/// The requested transition committed for the first time.
	Applied(AccountLifecycleMutation),
	/// The exact requested transition was already committed.
	Replayed(AccountLifecycleMutation),
	/// A deterministic guard rejected the transition.
	Rejected {
		/// Stable rejection class.
		rejection: AccountLifecycleRejection,
		/// Current account revision and operation phase.
		actual: AccountLifecycleMutation,
	},
}

/// Result of a revisioned rename or administrative enablement update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountAdministrationOutcome {
	/// The administrative projection is current.
	Updated {
		/// Current account revision.
		revision: i64,
	},
	/// A deterministic guard rejected the update.
	Rejected {
		/// Stable rejection class.
		rejection: AccountLifecycleRejection,
		/// Current account revision, or zero when no account exists.
		revision: i64,
	},
}

/// Result of one public routing-control mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoutingControlOutcome {
	/// The requested routing mutation committed.
	Updated {
		/// Complete routing projection at the new revision.
		routing: AccountRoutingControl,
	},
	/// The expected routing revision was stale.
	StaleRoutingControl {
		/// Current routing-control revision.
		revision: i64,
	},
	/// The fixed target's expected account revision was stale.
	StaleAccount {
		/// Current target-account revision.
		revision: i64,
	},
	/// The fixed target is not an enrolled, non-tombstoned routing member.
	AccountMissing,
	/// The requested order was not an exact visible-account permutation.
	InvalidOrder {
		/// Current routing-control revision.
		revision: i64,
	},
	/// The bounded command input was invalid.
	InvalidRequest,
}

/// Credential-negative result of one exact host-store observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountStoreObservation {
	/// The exact credential binding was observed.
	Exact,
	/// No credential item exists.
	Missing,
	/// Credential schema, version, fingerprint, or writer differs.
	Mismatch,
	/// The provider identity differs.
	ProviderMismatch,
	/// The host credential store could not be read.
	Unavailable,
}

/// Exact Codex build and generated/live callback profile persisted as readiness evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexAccountCapabilityAttestation {
	/// Stable Codex build identity.
	pub build_identity: String,
	/// Digest of the exact Codex executable.
	pub executable_sha256: String,
	/// Digest of the generated app-server schema.
	pub schema_sha256: String,
	/// Digest of the live refresh-callback profile.
	pub callback_profile_sha256: String,
	/// Whether initial ChatGPT token projection is available.
	pub login_chatgpt_auth_tokens: bool,
	/// Whether the required refresh callback is live.
	pub refresh_callback: bool,
}

/// Closed logical account command kind retained in durable request receipts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountCommandKind {
	/// Enroll from shared Codex credentials.
	Enroll,
	/// Import an explicit credential file.
	Import,
	/// Replace one account enablement switch.
	SetEnabled,
	/// Project one exact account into the shared Codex auth file.
	UseInCodex,
	/// Delete credentials and tombstone an account.
	Logout,
	/// Select one fixed account.
	SetFixedSelection,
	/// Select balanced account routing.
	SetBalancedSelection,
	/// Replace the complete account order.
	SetAccountOrder,
	/// Rotate one account credential bundle.
	Refresh,
	/// Reconcile one finite credential operation.
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

/// Owned durable command claim. The inner fencing token is not caller-visible.
pub struct AccountCommandReceiptLease(CommandReservation);

/// New receipt ownership or exact completed public result replay.
pub enum AccountCommandReceiptClaim {
	/// The caller owns the current fencing token.
	Owned(AccountCommandReceiptLease),
	/// The exact completed public result was replayed.
	Replayed(Value),
}

impl PostgresStore {
	/// Restore one complete credential-negative local account authority into a fresh latest schema.
	///
	/// This hidden schema-owner path never becomes part of the connected runtime store. The
	/// caller-owned fence runs after all rows are written but before readback and commit.
	#[cfg(unix)]
	#[doc(hidden)]
	pub async fn restore_local_account_authority_explicit<F>(
		config: &PostgresConnectionConfig,
		schema_owner: &PostgresIdentityConfig,
		schema_owner_password: Option<&str>,
		restore: &LocalAccountAuthorityRestore,
		precommit_fence: F,
	) -> Result<(), LocalAccountAuthorityRestoreFailure>
	where
		F: FnOnce() -> bool,
	{
		validate_local_account_authority_restore(restore)?;
		if schema_owner.user() == config.runtime().user() {
			return Err(local_restore_database(StoreError::UnsafeAuthority(
				"schema-owner and runtime PostgreSQL identities must be distinct",
			)));
		}

		let mut owner = connection_config(config, schema_owner, schema_owner_password);
		validate_connection(&owner).map_err(local_restore_database)?;
		let connector = verified_socket_connect(&owner, config.expected_peer_uid())
			.map_err(local_restore_database)?;
		apply_trusted_session_invariants(&mut owner);
		let manager = Manager::from_connect(
			owner,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager)
			.max_size(1)
			.build()
			.map_err(StoreError::from)
			.map_err(local_restore_database)?;
		let mut client = checkout(&pool, &connector).await.map_err(local_restore_database)?;
		let transaction = client
			.build_transaction()
			.isolation_level(IsolationLevel::Serializable)
			.start()
			.await
			.map_err(StoreError::from)
			.map_err(local_restore_database)?;
		let restore_result = restore_local_account_authority_transaction(
			&transaction,
			schema_owner.user(),
			config.runtime().user(),
			restore,
			precommit_fence,
		)
		.await;
		let result = match restore_result {
			Ok(()) =>
				transaction.commit().await.map_err(StoreError::from).map_err(local_restore_database),
			Err(error) => match transaction.rollback().await {
				Ok(()) => Err(error),
				Err(rollback_error) =>
					Err(local_restore_database(StoreError::from(rollback_error))),
			},
		};
		drop(client);
		pool.close();
		result
	}

	/// Reject the host-specific schema-owner restore on platforms without Unix sockets.
	#[cfg(not(unix))]
	#[doc(hidden)]
	pub async fn restore_local_account_authority_explicit<F>(
		_config: &PostgresConnectionConfig,
		_schema_owner: &PostgresIdentityConfig,
		_schema_owner_password: Option<&str>,
		_restore: &LocalAccountAuthorityRestore,
		_precommit_fence: F,
	) -> Result<(), LocalAccountAuthorityRestoreFailure>
	where
		F: FnOnce() -> bool,
	{
		Err(LocalAccountAuthorityRestoreFailure::Database(BootstrapFailure::Incompatible))
	}

	/// Read the complete non-tombstoned account skeleton and routing control in one snapshot.
	pub async fn read_account_registry_snapshot(
		&self,
		limit: u16,
	) -> Result<(Vec<AccountRecord>, AccountRoutingControl), StoreError> {
		if !(1..=512).contains(&limit) {
			return Err(StoreError::InvalidInput(
				"account registry limit must be between 1 and 512",
			));
		}
		let limit = i64::from(limit);
		let mut client = self.pool().get().await?;
		let transaction = client
			.build_transaction()
			.isolation_level(IsolationLevel::RepeatableRead)
			.start()
			.await?;
		let rows = transaction.query(READ_ACCOUNT_REGISTRY_ALL_SQL, &[&limit]).await?;
		let routing_row = transaction.query_one(READ_ACCOUNT_ROUTING_SQL, &[]).await?;
		let accounts = rows.into_iter().map(parse_account).collect::<Result<Vec<_>, _>>()?;
		let routing = parse_routing_control(&routing_row)?;
		validate_registry_snapshot(&accounts, &routing)?;
		transaction.commit().await?;

		Ok((accounts, routing))
	}

	/// Reserve or replay one credential-negative logical account command.
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
		let descriptor = CommandDescriptor {
			protocol_version: ACCOUNT_COMMAND_PROTOCOL,
			operation: kind.as_str(),
			project_scope: "global",
			scope_id: "accounts".to_owned(),
			entity_id: entity_id.to_owned(),
			expected_revision,
			payload_hash: None,
			payload_length: None,
		};
		let mut client = self.pool().get().await?;
		match reserve_command(&mut client, command, &descriptor).await? {
			CommandClaim::Owned(reservation) =>
				Ok(AccountCommandReceiptClaim::Owned(AccountCommandReceiptLease(reservation))),
			CommandClaim::Completed(response) => Ok(AccountCommandReceiptClaim::Replayed(response)),
		}
	}

	/// Complete one owned logical account command with its exact bounded public result.
	pub async fn complete_account_command(
		&self,
		lease: AccountCommandReceiptLease,
		result: &Value,
	) -> Result<(), StoreError> {
		validate_account_command_response(result)?;
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;
		finish_command(&transaction, &lease.0, result).await?;
		transaction.commit().await?;
		Ok(())
	}

	/// Read one or all credential-negative account projections.
	pub async fn read_account_registry(
		&self,
		account_id: Option<&AccountId>,
		limit: u16,
	) -> Result<Vec<AccountRecord>, StoreError> {
		if !(1..=512).contains(&limit) {
			return Err(StoreError::InvalidInput(
				"account registry limit must be between 1 and 512",
			));
		}
		let account_id = account_id.map(AccountId::as_str);
		let limit = i64::from(limit);
		let rows = self
			.pool()
			.get()
			.await?
			.query(READ_ACCOUNT_REGISTRY_SQL, &[&account_id, &limit])
			.await?;

		rows.into_iter().map(parse_account).collect()
	}

	/// Persist a finite account operation before any provider or Keychain effect.
	pub async fn prepare_account_operation(
		&self,
		preparation: &AccountOperationPreparation,
	) -> Result<AccountLifecycleMutationOutcome, StoreError> {
		if preparation.expected_account_revision.is_some_and(|revision| revision < 1) {
			return Err(StoreError::InvalidInput("expected account revision must be positive"));
		}
		let (expected_schema, expected_version, expected_fingerprint, expected_writer) =
			binding_parameters(preparation.expected.as_ref())?;
		let (target_schema, target_version, target_fingerprint, target_writer) =
			binding_parameters(preparation.target.as_ref())?;
		let row = self
			.pool()
			.get()
			.await?
			.query_one(
				PREPARE_ACCOUNT_OPERATION_SQL,
				&[
					&preparation.operation_id.as_str(),
					&preparation.account_id.as_str(),
					&operation_kind_text(preparation.kind),
					&preparation.display_label,
					&preparation.enabled,
					&preparation.expected_account_revision,
					&expected_schema,
					&expected_version,
					&expected_fingerprint,
					&expected_writer,
					&target_schema,
					&target_version,
					&target_fingerprint,
					&target_writer,
					&provider_text(preparation.provider.provider()),
					&preparation.provider.account_id(),
				],
			)
			.await?;

		parse_mutation_outcome(&row, true)
	}

	/// Advance one exact operation along an allowed finite transition.
	pub async fn advance_account_operation(
		&self,
		operation_id: &AccountOperationId,
		expected: AccountOperationPhase,
		target: AccountOperationPhase,
		recovery_code: Option<&str>,
	) -> Result<AccountLifecycleMutationOutcome, StoreError> {
		if recovery_code.is_some_and(|code| {
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
			return Err(StoreError::InvalidInput("account recovery code is invalid"));
		}
		let row = self
			.pool()
			.get()
			.await?
			.query_one(
				ADVANCE_ACCOUNT_OPERATION_SQL,
				&[
					&operation_id.as_str(),
					&operation_phase_text(expected),
					&operation_phase_text(target),
					&recovery_code,
				],
			)
			.await?;

		parse_mutation_outcome(&row, false)
	}

	/// Advance or replay one account operation and complete its exact logical-command result in the
	/// same PostgreSQL transaction.
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
			+ Send,
	{
		if recovery_code.is_some_and(|code| {
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
			return Err(StoreError::InvalidInput("account recovery code is invalid"));
		}
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;
		let row = transaction
			.query_one(
				ADVANCE_ACCOUNT_OPERATION_SQL,
				&[
					&operation_id.as_str(),
					&operation_phase_text(expected),
					&operation_phase_text(target),
					&recovery_code,
				],
			)
			.await?;
		let outcome = parse_mutation_outcome(&row, false)?;
		let operation = transaction
			.query_opt(READ_ACCOUNT_OPERATION_SQL, &[&operation_id.as_str()])
			.await?
			.map(parse_operation)
			.transpose()?;
		let account = match operation.as_ref() {
			Some(operation) => transaction
				.query_opt(READ_ACCOUNT_EXACT_SQL, &[&operation.account_id.as_str()])
				.await?
				.map(parse_account)
				.transpose()?,
			None => None,
		};
		let response = build_response(&outcome, operation.as_ref(), account.as_ref())?;
		validate_account_command_response(&response)?;
		finish_command(&transaction, &lease.0, &response).await?;
		transaction.commit().await?;
		Ok(response)
	}

	/// Attach the exact provider refresh result before the Keychain compare-and-swap effect.
	pub async fn set_account_operation_target(
		&self,
		operation_id: &AccountOperationId,
		target: &CredentialBinding,
	) -> Result<AccountLifecycleMutationOutcome, StoreError> {
		let schema = i32::from(target.schema_version.get());
		let version = i64::try_from(target.version.get()).map_err(|_| {
			StoreError::InvalidInput("credential version overflows PostgreSQL bigint")
		})?;
		let row = self
			.pool()
			.get()
			.await?
			.query_one(
				SET_ACCOUNT_OPERATION_TARGET_SQL,
				&[
					&operation_id.as_str(),
					&schema,
					&version,
					&target.fingerprint.as_str(),
					&target.writer_operation_id.as_str(),
				],
			)
			.await?;

		parse_mutation_outcome(&row, false)
	}

	/// Read every operation that still blocks admission, including typed manual recovery.
	pub async fn read_unsettled_account_operations(
		&self,
		limit: u16,
	) -> Result<Vec<AccountOperation>, StoreError> {
		if !(1..=512).contains(&limit) {
			return Err(StoreError::InvalidInput(
				"account operation limit must be between 1 and 512",
			));
		}
		let limit = i64::from(limit);
		let rows = self
			.pool()
			.get()
			.await?
			.query(READ_UNSETTLED_ACCOUNT_OPERATIONS_SQL, &[&limit])
			.await?;

		rows.into_iter().map(parse_operation).collect()
	}

	/// Read one exact lifecycle operation, including a terminal operation needed for replay.
	pub async fn read_account_operation(
		&self,
		operation_id: &AccountOperationId,
	) -> Result<Option<AccountOperation>, StoreError> {
		let row = self
			.pool()
			.get()
			.await?
			.query_opt(READ_ACCOUNT_OPERATION_SQL, &[&operation_id.as_str()])
			.await?;

		row.map(parse_operation).transpose()
	}

	/// Enable or disable one account at an exact registry revision.
	pub async fn set_account_enabled(
		&self,
		account_id: &AccountId,
		expected_revision: i64,
		enabled: bool,
	) -> Result<AccountAdministrationOutcome, StoreError> {
		if expected_revision < 1 {
			return Err(StoreError::InvalidInput("expected account revision must be positive"));
		}
		let row = self
			.pool()
			.get()
			.await?
			.query_one(
				SET_ACCOUNT_ENABLED_SQL,
				&[&account_id.as_str(), &expected_revision, &enabled],
			)
			.await?;
		let code: &str = row.get(0);
		let revision = row.get(1);
		match code {
			"updated" => Ok(AccountAdministrationOutcome::Updated { revision }),
			code => Ok(AccountAdministrationOutcome::Rejected {
				rejection: parse_rejection(code)?,
				revision,
			}),
		}
	}

	/// Apply one enablement mutation and complete its logical command receipt atomically.
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
			+ Send,
	{
		if expected_revision < 1 {
			return Err(StoreError::InvalidInput("expected account revision must be positive"));
		}
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;
		let row = transaction
			.query_one(
				SET_ACCOUNT_ENABLED_SQL,
				&[&account_id.as_str(), &expected_revision, &enabled],
			)
			.await?;
		let revision = row.get(1);
		let outcome = match row.get::<_, &str>(0) {
			"updated" => AccountAdministrationOutcome::Updated { revision },
			code => AccountAdministrationOutcome::Rejected {
				rejection: parse_rejection(code)?,
				revision,
			},
		};
		let account = if matches!(outcome, AccountAdministrationOutcome::Updated { .. }) {
			transaction
				.query_opt(READ_ACCOUNT_EXACT_SQL, &[&account_id.as_str()])
				.await?
				.map(parse_account)
				.transpose()?
		} else {
			None
		};
		let response = build_response(&outcome, account.as_ref())?;
		validate_account_command_response(&response)?;
		finish_command(&transaction, &lease.0, &response).await?;
		transaction.commit().await?;
		Ok(response)
	}

	/// Read fixed/balanced mode and the complete deterministic user order.
	pub async fn read_account_routing_control(&self) -> Result<AccountRoutingControl, StoreError> {
		let row = self.pool().get().await?.query_one(READ_ACCOUNT_ROUTING_SQL, &[]).await?;
		parse_routing_control(&row)
	}

	/// Select one fixed account under independent routing and account revision guards.
	pub async fn set_fixed_account_selection(
		&self,
		expected_routing_revision: i64,
		account_id: &AccountId,
		expected_account_revision: i64,
	) -> Result<RoutingControlOutcome, StoreError> {
		validate_routing_revision(expected_routing_revision)?;
		if expected_account_revision < 1 {
			return Err(StoreError::InvalidInput("expected account revision must be positive"));
		}
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;
		let row = transaction
			.query_one(
				SET_FIXED_ACCOUNT_SELECTION_SQL,
				&[&expected_routing_revision, &account_id.as_str(), &expected_account_revision],
			)
			.await?;
		let outcome = parse_routing_control_outcome(&transaction, &row, true).await?;
		transaction.commit().await?;
		Ok(outcome)
	}

	/// Select one fixed account and complete its logical command receipt atomically.
	pub async fn set_fixed_account_selection_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		expected_routing_revision: i64,
		account_id: &AccountId,
		expected_account_revision: i64,
		build_response: F,
	) -> Result<Value, StoreError>
	where
		F: FnOnce(&RoutingControlOutcome) -> Result<Value, StoreError> + Send,
	{
		validate_routing_revision(expected_routing_revision)?;
		if expected_account_revision < 1 {
			return Err(StoreError::InvalidInput("expected account revision must be positive"));
		}
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;
		let row = transaction
			.query_one(
				SET_FIXED_ACCOUNT_SELECTION_SQL,
				&[&expected_routing_revision, &account_id.as_str(), &expected_account_revision],
			)
			.await?;
		let outcome = parse_routing_control_outcome(&transaction, &row, true).await?;
		let response = build_response(&outcome)?;
		validate_account_command_response(&response)?;
		finish_command(&transaction, &lease.0, &response).await?;
		transaction.commit().await?;
		Ok(response)
	}

	/// Select balanced routing while preserving the complete account order.
	pub async fn set_balanced_account_selection(
		&self,
		expected_routing_revision: i64,
	) -> Result<RoutingControlOutcome, StoreError> {
		validate_routing_revision(expected_routing_revision)?;
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;
		let row = transaction
			.query_one(SET_BALANCED_ACCOUNT_SELECTION_SQL, &[&expected_routing_revision])
			.await?;
		let outcome = parse_routing_control_outcome(&transaction, &row, false).await?;
		transaction.commit().await?;
		Ok(outcome)
	}

	/// Select balanced routing and complete its logical command receipt atomically.
	pub async fn set_balanced_account_selection_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		expected_routing_revision: i64,
		build_response: F,
	) -> Result<Value, StoreError>
	where
		F: FnOnce(&RoutingControlOutcome) -> Result<Value, StoreError> + Send,
	{
		validate_routing_revision(expected_routing_revision)?;
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;
		let row = transaction
			.query_one(SET_BALANCED_ACCOUNT_SELECTION_SQL, &[&expected_routing_revision])
			.await?;
		let outcome = parse_routing_control_outcome(&transaction, &row, false).await?;
		let response = build_response(&outcome)?;
		validate_account_command_response(&response)?;
		finish_command(&transaction, &lease.0, &response).await?;
		transaction.commit().await?;
		Ok(response)
	}

	/// Replace the complete account order while preserving selection mode and fixed target.
	pub async fn set_account_order(
		&self,
		expected_routing_revision: i64,
		order: &[AccountId],
	) -> Result<RoutingControlOutcome, StoreError> {
		validate_routing_revision(expected_routing_revision)?;
		let order = routing_order_parameters(order)?;
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;
		let row = transaction
			.query_one(SET_ACCOUNT_ORDER_SQL, &[&expected_routing_revision, &order])
			.await?;
		let outcome = parse_routing_control_outcome(&transaction, &row, false).await?;
		transaction.commit().await?;
		Ok(outcome)
	}

	/// Replace the account order and complete its logical command receipt atomically.
	pub async fn set_account_order_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		expected_routing_revision: i64,
		order: &[AccountId],
		build_response: F,
	) -> Result<Value, StoreError>
	where
		F: FnOnce(&RoutingControlOutcome) -> Result<Value, StoreError> + Send,
	{
		validate_routing_revision(expected_routing_revision)?;
		let order = routing_order_parameters(order)?;
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;
		let row = transaction
			.query_one(SET_ACCOUNT_ORDER_SQL, &[&expected_routing_revision, &order])
			.await?;
		let outcome = parse_routing_control_outcome(&transaction, &row, false).await?;
		let response = build_response(&outcome)?;
		validate_account_command_response(&response)?;
		finish_command(&transaction, &lease.0, &response).await?;
		transaction.commit().await?;
		Ok(response)
	}

	/// Persist one of the two quota windows used for deterministic initial selection.
	pub async fn observe_account_quota(
		&self,
		account_id: &AccountId,
		fact: AccountQuotaWindow,
		observed_at_unix_micros: i64,
	) -> Result<(), StoreError> {
		let duration = i32::try_from(fact.duration_minutes)
			.map_err(|_| StoreError::InvalidInput("quota duration overflows"))?;
		let used = i32::from(fact.used_percent);
		let code: String = self
			.pool()
			.get()
			.await?
			.query_one(
				OBSERVE_ACCOUNT_QUOTA_SQL,
				&[
					&account_id.as_str(),
					&duration,
					&used,
					&fact.resets_at_unix_micros,
					&observed_at_unix_micros,
				],
			)
			.await?
			.get(0);
		if code == "observed" {
			Ok(())
		} else {
			Err(StoreError::InvalidInput("quota fact rejected"))
		}
	}

	/// Persist one bounded row-scoped quota observation failure.
	pub async fn observe_account_quota_error(
		&self,
		account_id: &AccountId,
		duration_minutes: u32,
		error: AccountQuotaObservationError,
		observed_at_unix_micros: i64,
	) -> Result<(), StoreError> {
		let duration = i32::try_from(duration_minutes)
			.map_err(|_| StoreError::InvalidInput("quota duration overflows"))?;
		let code: String = self
			.pool()
			.get()
			.await?
			.query_one(
				OBSERVE_ACCOUNT_QUOTA_ERROR_SQL,
				&[
					&account_id.as_str(),
					&duration,
					&quota_error_text(error),
					&observed_at_unix_micros,
				],
			)
			.await?
			.get(0);
		if code == "observed" {
			Ok(())
		} else {
			Err(StoreError::InvalidInput("quota error rejected"))
		}
	}

	/// Persist the last exact host-store observation against one unchanged registry binding.
	pub async fn observe_account_store(
		&self,
		account_id: &AccountId,
		expected_revision: i64,
		expected: &CredentialBinding,
		observation: AccountStoreObservation,
	) -> Result<bool, StoreError> {
		let schema = i32::from(expected.schema_version.get());
		let version = i64::try_from(expected.version.get()).map_err(|_| {
			StoreError::InvalidInput("credential version overflows PostgreSQL bigint")
		})?;
		let code: String = self
			.pool()
			.get()
			.await?
			.query_one(
				OBSERVE_ACCOUNT_STORE_SQL,
				&[
					&account_id.as_str(),
					&expected_revision,
					&schema,
					&version,
					&expected.fingerprint.as_str(),
					&expected.writer_operation_id.as_str(),
					&provider_text(expected.provider.provider()),
					&expected.provider.account_id(),
					&store_observation_text(observation),
				],
			)
			.await?
			.get(0);
		match code.as_str() {
			"observed" => Ok(true),
			"stale_account" => Ok(false),
			_ => Err(incompatible("account store observation result")),
		}
	}

	/// Persist exact-build generated/live callback readiness. Unsupported facts remain unready.
	pub async fn attest_codex_account_capability(
		&self,
		attestation: &CodexAccountCapabilityAttestation,
	) -> Result<bool, StoreError> {
		let code: String = self
			.pool()
			.get()
			.await?
			.query_one(
				ATTEST_CODEX_ACCOUNT_CAPABILITY_SQL,
				&[
					&attestation.build_identity,
					&attestation.executable_sha256,
					&attestation.schema_sha256,
					&attestation.callback_profile_sha256,
					&attestation.login_chatgpt_auth_tokens,
					&attestation.refresh_callback,
				],
			)
			.await?
			.get(0);
		Ok(code == "ready")
	}
}

fn validate_local_account_authority_restore(
	restore: &LocalAccountAuthorityRestore,
) -> Result<(), LocalAccountAuthorityRestoreFailure> {
	let accounts = &restore.accounts;
	if accounts.len() > LOCAL_ACCOUNT_AUTHORITY_MAX_ACCOUNTS || restore.routing.revision < 1 {
		return Err(LocalAccountAuthorityRestoreFailure::InvalidInput);
	}
	let account_ids =
		accounts.iter().map(|account| account.account_id.clone()).collect::<BTreeSet<_>>();
	let provider_ids = accounts
		.iter()
		.map(|account| {
			(
				provider_text(account.credential.provider.provider()),
				account.credential.provider.account_id().to_owned(),
			)
		})
		.collect::<BTreeSet<_>>();
	let writer_ids = accounts
		.iter()
		.map(|account| account.credential.writer_operation_id.clone())
		.collect::<BTreeSet<_>>();
	if account_ids.len() != accounts.len()
		|| provider_ids.len() != accounts.len()
		|| writer_ids.len() != accounts.len()
		|| accounts.iter().map(|account| &account.account_id).ne(restore.routing.order.iter())
		|| accounts.iter().any(|account| {
			account.revision < 1
				|| account.display_label.is_empty()
				|| account.display_label.len() > 128
				|| account
					.display_label
					.chars()
					.any(|character| character.is_control() || character.is_whitespace())
				|| account.credential.schema_version != CredentialStoreSchemaVersion::V1
				|| account.credential.version.get() > i64::MAX as u64
		}) {
		return Err(LocalAccountAuthorityRestoreFailure::InvalidInput);
	}
	if let AccountSelectionMode::Fixed(account_id) = &restore.routing.mode
		&& !account_ids.contains(account_id)
	{
		return Err(LocalAccountAuthorityRestoreFailure::InvalidInput);
	}

	Ok(())
}

fn local_restore_database(error: StoreError) -> LocalAccountAuthorityRestoreFailure {
	LocalAccountAuthorityRestoreFailure::Database(error.bootstrap_failure())
}

#[cfg(unix)]
async fn restore_local_account_authority_transaction<F>(
	transaction: &Transaction<'_>,
	schema_owner_role: &str,
	runtime_role: &str,
	restore: &LocalAccountAuthorityRestore,
	precommit_fence: F,
) -> Result<(), LocalAccountAuthorityRestoreFailure>
where
	F: FnOnce() -> bool,
{
	verify_local_restore_authority(transaction, schema_owner_role, runtime_role)
		.await
		.map_err(local_restore_database)?;
	let relations =
		local_restore_relation_inventory(transaction).await.map_err(local_restore_database)?;
	lock_local_restore_relations(transaction, &relations).await.map_err(local_restore_database)?;
	if !local_restore_target_is_fresh(transaction, &relations)
		.await
		.map_err(local_restore_database)?
	{
		return Err(LocalAccountAuthorityRestoreFailure::TargetNotFresh);
	}

	for account in &restore.accounts {
		let credential_version = i64::try_from(account.credential.version.get())
			.map_err(|_| LocalAccountAuthorityRestoreFailure::InvalidInput)?;
		let credential_store_schema_version = i32::from(account.credential.schema_version.get());
		transaction
			.execute(
				RESTORE_LOCAL_ACCOUNT_SQL,
				&[
					&account.account_id.as_str(),
					&account.display_label,
					&account.revision,
					&account.enabled,
					&provider_text(account.credential.provider.provider()),
					&account.credential.provider.account_id(),
					&credential_store_schema_version,
					&credential_version,
					&account.credential.fingerprint.as_str(),
					&account.credential.writer_operation_id.as_str(),
				],
			)
			.await
			.map_err(StoreError::from)
			.map_err(local_restore_database)?;
	}
	for (position, account_id) in restore.routing.order.iter().enumerate() {
		let position = i32::try_from(position)
			.map_err(|_| LocalAccountAuthorityRestoreFailure::InvalidInput)?;
		transaction
			.execute(
				"INSERT INTO decodex.account_routing_order(account_id,position) \
				 VALUES($1::text::uuid,$2)",
				&[&account_id.as_str(), &position],
			)
			.await
			.map_err(StoreError::from)
			.map_err(local_restore_database)?;
	}
	let (mode, fixed_account_id) = match &restore.routing.mode {
		AccountSelectionMode::Fixed(account_id) => ("fixed", Some(account_id.as_str())),
		AccountSelectionMode::Balanced => ("balanced", None),
	};
	transaction
		.execute(
			"UPDATE decodex.account_routing_control SET mode=$1::text::decodex.account_selection_mode,\
			 fixed_account_id=$2::text::uuid,revision=$3,updated_at=pg_catalog.clock_timestamp() \
			 WHERE singleton",
			&[&mode, &fixed_account_id, &restore.routing.revision],
		)
		.await
		.map_err(StoreError::from)
		.map_err(local_restore_database)?;
	transaction
		.query_one("SELECT decodex.lock_account_routing_universe_exact()", &[])
		.await
		.map_err(StoreError::from)
		.map_err(local_restore_database)?;

	if !precommit_fence() {
		return Err(LocalAccountAuthorityRestoreFailure::PrecommitFence);
	}
	transaction
		.execute(
			"UPDATE decodex.accounts SET \
			 credential_store_observed_at=pg_catalog.clock_timestamp()",
			&[],
		)
		.await
		.map_err(StoreError::from)
		.map_err(local_restore_database)?;
	if !local_restore_readback_matches(transaction, restore)
		.await
		.map_err(local_restore_database)?
		|| !local_restore_unrelated_state_is_empty(transaction, &relations, restore.accounts.len())
			.await
			.map_err(local_restore_database)?
	{
		return Err(LocalAccountAuthorityRestoreFailure::ReadbackMismatch);
	}
	verify_local_restore_authority(transaction, schema_owner_role, runtime_role)
		.await
		.map_err(local_restore_database)?;

	Ok(())
}

#[cfg(unix)]
async fn verify_local_restore_authority<C>(
	client: &C,
	schema_owner_role: &str,
	runtime_role: &str,
) -> Result<(), StoreError>
where
	C: GenericClient + Sync,
{
	let exact_owner: bool = client
		.query_one(
			"SELECT session_user=$1::pg_catalog.name AND current_user=$1::pg_catalog.name \
			 AND database_owner.rolname=$1::pg_catalog.name FROM pg_catalog.pg_database AS database \
			 JOIN pg_catalog.pg_roles AS database_owner ON database_owner.oid=database.datdba \
			 WHERE database.datname=pg_catalog.current_database()",
			&[&schema_owner_role],
		)
		.await?
		.get(0);
	if !exact_owner {
		return Err(StoreError::UnsafeAuthority(
			"local account restore identity is not the database schema owner",
		));
	}
	schema::verify_platform(client).await?;
	let evidence =
		authority::bootstrap_authority_evidence(client, schema_owner_role, runtime_role).await?;
	authority::enforce_bootstrap_authority(&evidence)
}

#[cfg(unix)]
async fn local_restore_relation_inventory<C>(client: &C) -> Result<Vec<String>, StoreError>
where
	C: GenericClient + Sync,
{
	let relations = client
		.query(
			"SELECT relation.relname::text FROM pg_catalog.pg_class AS relation \
			 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=relation.relnamespace \
			 WHERE namespace.nspname='decodex' AND relation.relkind IN ('r','p') \
			 ORDER BY relation.relname",
			&[],
		)
		.await?
		.into_iter()
		.map(|row| row.get::<_, String>(0))
		.collect::<Vec<_>>();
	if relations.is_empty()
		|| relations.len() > 256
		|| relations.iter().any(|relation| !is_local_restore_identifier(relation))
	{
		return Err(incompatible("local account restore relation inventory"));
	}
	Ok(relations)
}

#[cfg(unix)]
async fn lock_local_restore_relations(
	transaction: &Transaction<'_>,
	relations: &[String],
) -> Result<(), StoreError> {
	for relation in relations {
		transaction
			.batch_execute(&format!("LOCK TABLE decodex.{relation} IN ACCESS EXCLUSIVE MODE"))
			.await?;
	}
	Ok(())
}

#[cfg(unix)]
async fn local_restore_target_is_fresh(
	transaction: &Transaction<'_>,
	relations: &[String],
) -> Result<bool, StoreError> {
	let routing_is_initial: bool = transaction
		.query_one(
			"SELECT pg_catalog.count(*)=1 AND pg_catalog.bool_and(singleton AND mode='balanced' \
			 AND fixed_account_id IS NULL AND revision=1) \
			 FROM decodex.account_routing_control",
			&[],
		)
		.await?
		.get(0);
	let execution_epoch_is_initial: bool = transaction
		.query_one(
			"SELECT pg_catalog.count(*)=1 AND pg_catalog.bool_and(retired_at IS NULL) \
			 FROM decodex.process_generation_execution_epochs",
			&[],
		)
		.await?
		.get(0);
	if !routing_is_initial || !execution_epoch_is_initial {
		return Ok(false);
	}
	for relation in relations {
		if matches!(
			relation.as_str(),
			"account_routing_control" | "process_generation_execution_epochs"
		) {
			continue;
		}
		let empty: bool = transaction
			.query_one(&format!("SELECT NOT EXISTS (SELECT 1 FROM decodex.{relation})"), &[])
			.await?
			.get(0);
		if !empty {
			return Ok(false);
		}
	}
	local_restore_sequences_are_initial(transaction).await
}

#[cfg(unix)]
async fn local_restore_sequences_are_initial<C>(client: &C) -> Result<bool, StoreError>
where
	C: GenericClient + Sync,
{
	let sequences = client
		.query(
			"SELECT relation.relname::text FROM pg_catalog.pg_class AS relation \
			 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=relation.relnamespace \
			 WHERE namespace.nspname='decodex' AND relation.relkind='S' \
			 ORDER BY relation.relname",
			&[],
		)
		.await?
		.into_iter()
		.map(|row| row.get::<_, String>(0))
		.collect::<Vec<_>>();
	if sequences.is_empty()
		|| sequences.len() > 16
		|| sequences.iter().any(|sequence| !is_local_restore_identifier(sequence))
	{
		return Err(incompatible("local account restore sequence inventory"));
	}
	for sequence in sequences {
		let initial: bool = client
			.query_one(
				&format!("SELECT last_value=1 AND NOT is_called FROM decodex.{sequence}"),
				&[],
			)
			.await?
			.get(0);
		if !initial {
			return Ok(false);
		}
	}
	Ok(true)
}

#[cfg(unix)]
async fn local_restore_readback_matches(
	transaction: &Transaction<'_>,
	restore: &LocalAccountAuthorityRestore,
) -> Result<bool, StoreError> {
	let rows = transaction.query(READ_RESTORED_LOCAL_ACCOUNTS_SQL, &[]).await?;
	if rows.len() != restore.accounts.len() {
		return Ok(false);
	}
	for (row, account) in rows.iter().zip(&restore.accounts) {
		let credential_version = i64::try_from(account.credential.version.get()).map_err(|_| {
			StoreError::InvalidInput("credential version overflows PostgreSQL bigint")
		})?;
		if row.get::<_, &str>(0) != account.account_id.as_str()
			|| row.get::<_, &str>(1) != account.display_label.as_str()
			|| row.get::<_, bool>(2) != account.enabled
			|| row.get::<_, i64>(3) != account.revision
			|| row.get::<_, &str>(4) != provider_text(account.credential.provider.provider())
			|| row.get::<_, &str>(5) != account.credential.provider.account_id()
			|| row.get::<_, i32>(6) != i32::from(account.credential.schema_version.get())
			|| row.get::<_, i64>(7) != credential_version
			|| row.get::<_, &str>(8) != account.credential.fingerprint.as_str()
			|| row.get::<_, &str>(9) != account.credential.writer_operation_id.as_str()
			|| row.get::<_, &str>(10) != "exact"
			|| !row.get::<_, bool>(11)
			|| row.get::<_, bool>(12)
			|| row.get::<_, &str>(13) != "unknown"
			|| row.get::<_, Value>(14) != Value::Object(Default::default())
		{
			return Ok(false);
		}
	}
	let routing =
		parse_routing_control(&transaction.query_one(READ_ACCOUNT_ROUTING_SQL, &[]).await?)?;
	Ok(routing == restore.routing)
}

#[cfg(unix)]
async fn local_restore_unrelated_state_is_empty(
	transaction: &Transaction<'_>,
	relations: &[String],
	expected_account_count: usize,
) -> Result<bool, StoreError> {
	let expected_account_count = i64::try_from(expected_account_count)
		.map_err(|_| StoreError::InvalidInput("local account restore count is invalid"))?;
	let account_shape: bool = transaction
		.query_one(
			"SELECT (SELECT pg_catalog.count(*) FROM decodex.accounts)=$1 \
			 AND (SELECT pg_catalog.count(*) FROM decodex.account_routing_order)=$1 \
			 AND NOT EXISTS (SELECT 1 FROM decodex.accounts WHERE tombstoned_at IS NOT NULL)",
			&[&expected_account_count],
		)
		.await?
		.get(0);
	if !account_shape {
		return Ok(false);
	}
	for relation in relations {
		if matches!(
			relation.as_str(),
			"accounts"
				| "account_routing_control"
				| "account_routing_order"
				| "process_generation_execution_epochs"
		) {
			continue;
		}
		let empty: bool = transaction
			.query_one(&format!("SELECT NOT EXISTS (SELECT 1 FROM decodex.{relation})"), &[])
			.await?
			.get(0);
		if !empty {
			return Ok(false);
		}
	}
	local_restore_sequences_are_initial(transaction).await
}

#[cfg(unix)]
fn is_local_restore_identifier(value: &str) -> bool {
	let mut bytes = value.bytes();
	bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
		&& bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(all(test, feature = "test-support"))]
pub(crate) async fn prepare_account_lifecycle_sql(
	client: &tokio_postgres::Client,
) -> Result<usize, StoreError> {
	const SOURCES: [&str; 19] = [
		READ_ACCOUNT_REGISTRY_ALL_SQL,
		READ_ACCOUNT_REGISTRY_SQL,
		READ_ACCOUNT_EXACT_SQL,
		READ_ACCOUNT_ROUTING_SQL,
		PREPARE_ACCOUNT_OPERATION_SQL,
		ADVANCE_ACCOUNT_OPERATION_SQL,
		SET_ACCOUNT_OPERATION_TARGET_SQL,
		READ_UNSETTLED_ACCOUNT_OPERATIONS_SQL,
		READ_ACCOUNT_OPERATION_SQL,
		SET_ACCOUNT_ENABLED_SQL,
		SET_FIXED_ACCOUNT_SELECTION_SQL,
		SET_BALANCED_ACCOUNT_SELECTION_SQL,
		SET_ACCOUNT_ORDER_SQL,
		OBSERVE_ACCOUNT_QUOTA_SQL,
		OBSERVE_ACCOUNT_QUOTA_ERROR_SQL,
		OBSERVE_ACCOUNT_STORE_SQL,
		ATTEST_CODEX_ACCOUNT_CAPABILITY_SQL,
		RESTORE_LOCAL_ACCOUNT_SQL,
		READ_RESTORED_LOCAL_ACCOUNTS_SQL,
	];
	for source in SOURCES {
		client.prepare(source).await?;
	}
	Ok(SOURCES.len())
}

fn parse_account(row: tokio_postgres::Row) -> Result<AccountRecord, StoreError> {
	let account_id = parse_account_id(row.get::<_, String>(0))?;
	let label: String = row.get(1);
	let enabled: bool = row.get(2);
	let observed_state = parse_account_state(row.get(3))?;
	let revision: i64 = row.get(4);
	let provider = parse_optional_provider(row.get(5), row.get(6))?;
	let credential =
		parse_optional_binding(row.get(7), row.get(8), row.get(9), row.get(10), provider.as_ref())?;
	let lifecycle_readiness = parse_lifecycle_readiness(row.get(12))?;
	let unsettled_operation =
		parse_operation_status(row.get(13), row.get(14), row.get(15), row.get(16))?;
	let five_hour_quota = parse_quota(
		AccountQuotaWindow::FIVE_HOURS_MINUTES,
		row.get(17),
		row.get(18),
		row.get(19),
		row.get(20),
		row.get(21),
	)?;
	let seven_day_quota = parse_quota(
		AccountQuotaWindow::SEVEN_DAYS_MINUTES,
		row.get(22),
		row.get(23),
		row.get(24),
		row.get(25),
		row.get(26),
	)?;
	if revision < 1 || label.is_empty() || label.len() > 128 {
		return Err(incompatible("account registry projection"));
	}
	Ok(AccountRecord {
		account_id,
		label,
		enabled,
		revision,
		observed_state,
		lifecycle_readiness,
		credential,
		unsettled_operation,
		five_hour_quota,
		seven_day_quota,
		tombstoned: row.get(11),
	})
}

fn parse_routing_control(row: &tokio_postgres::Row) -> Result<AccountRoutingControl, StoreError> {
	let mode = match row.get::<_, &str>(0) {
		"fixed" => AccountSelectionMode::Fixed(parse_account_id(
			row.get::<_, Option<String>>(1)
				.ok_or_else(|| incompatible("fixed account identity"))?,
		)?),
		"balanced" => AccountSelectionMode::Balanced,
		_ => return Err(incompatible("account selection mode")),
	};
	let revision: i64 = row.get(2);
	let order = row
		.get::<_, Vec<String>>(3)
		.into_iter()
		.map(parse_account_id)
		.collect::<Result<Vec<_>, _>>()?;
	if revision < 1 {
		return Err(incompatible("account routing revision"));
	}
	let members = order.iter().cloned().collect::<BTreeSet<_>>();
	if members.len() != order.len() {
		return Err(incompatible("account routing order"));
	}
	if let AccountSelectionMode::Fixed(account_id) = &mode
		&& !members.contains(account_id)
	{
		return Err(incompatible("fixed account routing target"));
	}

	Ok(AccountRoutingControl { revision, mode, order })
}

async fn parse_routing_control_outcome(
	transaction: &tokio_postgres::Transaction<'_>,
	row: &tokio_postgres::Row,
	fixed_selection: bool,
) -> Result<RoutingControlOutcome, StoreError> {
	let routing_revision: i64 = row.get(1);
	let routing_row = transaction.query_one(READ_ACCOUNT_ROUTING_SQL, &[]).await?;
	let routing = parse_routing_control(&routing_row)?;
	if routing.revision != routing_revision {
		return Err(incompatible("account routing mutation revision"));
	}
	match row.get::<_, &str>(0) {
		"updated" => Ok(RoutingControlOutcome::Updated { routing }),
		"stale_routing_control" =>
			Ok(RoutingControlOutcome::StaleRoutingControl { revision: routing_revision }),
		"stale_account" if fixed_selection => {
			let revision: i64 = row.get(2);
			if revision < 1 {
				return Err(incompatible("fixed account revision"));
			}
			Ok(RoutingControlOutcome::StaleAccount { revision })
		},
		"account_missing" if fixed_selection => Ok(RoutingControlOutcome::AccountMissing),
		"invalid_order" if !fixed_selection =>
			Ok(RoutingControlOutcome::InvalidOrder { revision: routing_revision }),
		"invalid_request" => Ok(RoutingControlOutcome::InvalidRequest),
		_ => Err(incompatible("account routing result")),
	}
}

fn validate_routing_revision(revision: i64) -> Result<(), StoreError> {
	if revision < 1 {
		Err(StoreError::InvalidInput("expected routing revision must be positive"))
	} else {
		Ok(())
	}
}

fn routing_order_parameters(order: &[AccountId]) -> Result<Vec<String>, StoreError> {
	if order.len() > 512 || order.iter().cloned().collect::<BTreeSet<_>>().len() != order.len() {
		return Err(StoreError::InvalidInput("account routing order is invalid"));
	}
	Ok(order.iter().map(|account_id| account_id.as_str().to_owned()).collect())
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

fn parse_operation_status(
	operation_id: Option<String>,
	kind: Option<&str>,
	phase: Option<&str>,
	recovery_code: Option<String>,
) -> Result<Option<AccountOperationStatus>, StoreError> {
	match (operation_id, kind, phase) {
		(None, None, None) if recovery_code.is_none() => Ok(None),
		(Some(operation_id), Some(kind), Some(phase)) => Ok(Some(AccountOperationStatus {
			operation_id: AccountOperationId::new(operation_id)
				.map_err(|_| incompatible("account operation identity"))?,
			kind: parse_operation_kind(kind)?,
			phase: parse_operation_phase(phase)?,
			recovery_code,
		})),
		_ => Err(incompatible("partial unsettled account operation")),
	}
}

fn parse_operation(row: tokio_postgres::Row) -> Result<AccountOperation, StoreError> {
	let operation_id = AccountOperationId::new(row.get::<_, String>(0))
		.map_err(|_| incompatible("account operation identity"))?;
	let account_id = parse_account_id(row.get(1))?;
	let kind = parse_operation_kind(row.get(2))?;
	let phase = parse_operation_phase(row.get(3))?;
	let provider = parse_optional_provider(row.get(15), row.get(16))?
		.ok_or_else(|| incompatible("account operation provider"))?;
	let expected =
		parse_optional_binding(row.get(7), row.get(8), row.get(9), row.get(10), Some(&provider))?;
	let target = parse_optional_binding(
		row.get(11),
		row.get(12),
		row.get(13),
		row.get(14),
		Some(&provider),
	)?;
	Ok(AccountOperation {
		operation_id,
		account_id,
		kind,
		phase,
		expected_account_revision: row.get(4),
		requested_display_label: row.get(5),
		requested_enabled: row.get(6),
		expected,
		target,
	})
}

type OptionalBindingParameters<'a> = (Option<i32>, Option<i64>, Option<&'a str>, Option<&'a str>);

fn binding_parameters(
	binding: Option<&CredentialBinding>,
) -> Result<OptionalBindingParameters<'_>, StoreError> {
	match binding {
		Some(binding) => Ok((
			Some(i32::from(binding.schema_version.get())),
			Some(i64::try_from(binding.version.get()).map_err(|_| {
				StoreError::InvalidInput("credential version overflows PostgreSQL bigint")
			})?),
			Some(binding.fingerprint.as_str()),
			Some(binding.writer_operation_id.as_str()),
		)),
		None => Ok((None, None, None, None)),
	}
}

fn parse_optional_binding(
	schema: Option<i32>,
	version: Option<i64>,
	fingerprint: Option<String>,
	writer_operation_id: Option<String>,
	provider: Option<&ProviderIdentity>,
) -> Result<Option<CredentialBinding>, StoreError> {
	match (schema, version, fingerprint, writer_operation_id, provider) {
		(None, None, None, None, _) => Ok(None),
		(
			Some(schema),
			Some(version),
			Some(fingerprint),
			Some(writer_operation_id),
			Some(provider),
		) => Ok(Some(CredentialBinding {
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
			provider: provider.clone(),
			writer_operation_id: AccountOperationId::new(writer_operation_id)
				.map_err(|_| incompatible("credential writer operation identity"))?,
		})),
		_ => Err(incompatible("partial credential binding")),
	}
}

fn parse_optional_provider(
	kind: Option<&str>,
	account_id: Option<String>,
) -> Result<Option<ProviderIdentity>, StoreError> {
	match (kind, account_id) {
		(None, None) => Ok(None),
		(Some("chatgpt"), Some(account_id)) =>
			ProviderIdentity::new(AccountProvider::Chatgpt, account_id)
				.map(Some)
				.map_err(|_| incompatible("provider identity")),
		_ => Err(incompatible("partial provider identity")),
	}
}

fn parse_quota(
	duration: u32,
	status: &str,
	used: Option<i32>,
	resets_micros: Option<i64>,
	observed_micros: Option<i64>,
	error: Option<&str>,
) -> Result<AccountQuotaWindowObservation, StoreError> {
	let disposition = match (status, used, resets_micros, error) {
		("unknown", None, None, None) if observed_micros.is_none() =>
			AccountQuotaDisposition::Unknown,
		("current", Some(used), Some(resets), None) => AccountQuotaDisposition::Current(
			AccountQuotaWindow::new(
				duration,
				u8::try_from(used).map_err(|_| incompatible("quota percentage"))?,
				resets,
			)
			.map_err(|_| incompatible("quota window"))?,
		),
		("stale", Some(used), Some(resets), None) => AccountQuotaDisposition::Stale(
			AccountQuotaWindow::new(
				duration,
				u8::try_from(used).map_err(|_| incompatible("quota percentage"))?,
				resets,
			)
			.map_err(|_| incompatible("quota window"))?,
		),
		("error", None, None, Some(error)) =>
			AccountQuotaDisposition::Error(parse_quota_error(error)?),
		_ => return Err(incompatible("quota observation shape")),
	};
	Ok(AccountQuotaWindowObservation {
		duration_minutes: duration,
		observed_at_unix_micros: observed_micros,
		disposition,
	})
}

fn parse_mutation_outcome(
	row: &tokio_postgres::Row,
	prepare: bool,
) -> Result<AccountLifecycleMutationOutcome, StoreError> {
	let code: &str = row.get(0);
	let actual = AccountLifecycleMutation {
		account_revision: row.get(1),
		phase: parse_operation_phase(row.get(2))?,
	};
	if code == "replayed" {
		return Ok(AccountLifecycleMutationOutcome::Replayed(actual));
	}
	if (prepare && code == "prepared") || (!prepare && code == "advanced") {
		return Ok(AccountLifecycleMutationOutcome::Applied(actual));
	}
	Ok(AccountLifecycleMutationOutcome::Rejected { rejection: parse_rejection(code)?, actual })
}

fn parse_rejection(value: &str) -> Result<AccountLifecycleRejection, StoreError> {
	match value {
		"identity_conflict" => Ok(AccountLifecycleRejection::IdentityConflict),
		"operation_unsettled" => Ok(AccountLifecycleRejection::OperationUnsettled),
		"invalid_request" => Ok(AccountLifecycleRejection::InvalidRequest),
		"account_missing" => Ok(AccountLifecycleRejection::AccountMissing),
		"stale_account" => Ok(AccountLifecycleRejection::StaleAccount),
		"account_in_use" => Ok(AccountLifecycleRejection::AccountInUse),
		"operation_missing" => Ok(AccountLifecycleRejection::OperationMissing),
		"stale_operation" => Ok(AccountLifecycleRejection::StaleOperation),
		_ => Err(incompatible("account lifecycle result")),
	}
}

fn parse_account_id(value: String) -> Result<AccountId, StoreError> {
	AccountId::new(value).map_err(|_| incompatible("account identity"))
}

fn parse_account_state(value: &str) -> Result<AccountState, StoreError> {
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

fn parse_lifecycle_readiness(value: &str) -> Result<AccountLifecycleReadiness, StoreError> {
	match value {
		"ready" => Ok(AccountLifecycleReadiness::Ready),
		"credential_absent" => Ok(AccountLifecycleReadiness::CredentialAbsent),
		"store_unavailable" => Ok(AccountLifecycleReadiness::StoreUnavailable),
		"store_mismatch" => Ok(AccountLifecycleReadiness::StoreMismatch),
		"provider_mismatch" => Ok(AccountLifecycleReadiness::ProviderMismatch),
		"operation_unsettled" => Ok(AccountLifecycleReadiness::OperationUnsettled),
		"callback_capability_unready" => Ok(AccountLifecycleReadiness::CallbackCapabilityUnready),
		"tombstoned" => Ok(AccountLifecycleReadiness::Tombstoned),
		_ => Err(incompatible("account lifecycle readiness")),
	}
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
const fn provider_text(value: AccountProvider) -> &'static str {
	match value {
		AccountProvider::Chatgpt => "chatgpt",
	}
}
const fn store_observation_text(value: AccountStoreObservation) -> &'static str {
	match value {
		AccountStoreObservation::Exact => "exact",
		AccountStoreObservation::Missing => "missing",
		AccountStoreObservation::Mismatch => "mismatch",
		AccountStoreObservation::ProviderMismatch => "provider_mismatch",
		AccountStoreObservation::Unavailable => "unavailable",
	}
}
const fn quota_error_text(value: AccountQuotaObservationError) -> &'static str {
	match value {
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
fn validate_account_command_response(value: &Value) -> Result<(), StoreError> {
	let bytes = serde_json::to_vec(value)
		.map_err(|_| StoreError::InvalidInput("account command result is invalid"))?;
	if bytes.len() > 256 * 1024 {
		return Err(StoreError::InvalidInput("account command result is invalid"));
	}
	crate::ensure_credential_negative_json(value)
}
fn incompatible(value: &'static str) -> StoreError {
	StoreError::Incompatible(format!("stored {value} is malformed"))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn expired_quota_remains_stale_inside_the_store_boundary() {
		let observation = parse_quota(
			AccountQuotaWindow::FIVE_HOURS_MINUTES,
			"stale",
			Some(42),
			Some(2_000_000),
			Some(1_000_000),
			None,
		)
		.expect("valid expired quota should remain readable");

		assert_eq!(observation.observed_at_unix_micros, Some(1_000_000));
		assert!(matches!(
			observation.disposition,
			AccountQuotaDisposition::Stale(AccountQuotaWindow {
				used_percent: 42,
				resets_at_unix_micros: 2_000_000,
				..
			})
		));
	}
}
