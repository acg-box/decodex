//! Durable preparation and reconciliation state for the manual reset-card operation.
//!
//! The existing outbox already provides the required effect fence, claim lease, receipt,
//! readback, and retention states. This module gives one narrow operation a typed projection over
//! that ledger. It does not add a second side-effect store or expose provider identifiers through
//! public activity and command responses.

use std::time::Duration;

use serde_json::{Value, json};
use tokio_postgres::Row;

use crate::{
	AccountId, AccountState, CommandIdentity, PostgresStore, StoreError,
	accounts::{
		CommandClaim, CommandDescriptor, CommandReplay, CommandReservation,
		append_activity_and_outbox, finish_command, inspect_command_receipt, reserve_command,
	},
};
use decodex_core::{
	AccountOperationId, AccountProvider, CredentialBinding, CredentialFingerprint,
	CredentialStoreSchemaVersion, CredentialVersion, ProcessGenerationAccountBinding,
	ProviderIdentity, ResetCardConsumeOutcome, ResetCardDescriptor, ResetCardTimestamp,
	admit_manual_reset_card_use,
};

const PROTOCOL: &str = "decodex/reset-card-operation/1";
const STORE_COMMAND_PROTOCOL: &str = "decodex/store-command/1";
const AGGREGATE_KIND: &str = "reset_card_operation";
const EVENT_KIND: &str = "reset_card_operation_prepared";
const MAX_EXACT_CREDIT_ID_BYTES: usize = 1_024;
const MAX_PRIVATE_PROVIDER_KEY_BYTES: usize = 1_024;
const PRIVATE_EFFECT_FIELD: &str = "reset_card_effect";
const PRIVATE_PROVIDER_KEY_FIELD: &str = "provider_idempotency_key_hex";
const PRIVATE_CREDIT_ID_FIELD: &str = "provider_credit_id_hex";
const PRIVATE_WRITER_OPERATION_ID_FIELD: &str = "credential_writer_operation_id";
const ACCOUNT_LIFECYCLE_LOCK_SQL: &str =
	"SELECT pg_catalog.pg_advisory_xact_lock(1422,pg_catalog.hashtext($1))";
const RESET_CARD_ACCOUNT_ADMISSION_SQL: &str = "SELECT state::text,revision,enabled,tombstoned, \
	 credential_store_schema_version,credential_version,credential_fingerprint, \
	 credential_writer_operation_id::text,provider_kind::text,provider_account_id, \
	 credential_store_observation::text,operation_unsettled,callback_profile_ready \
	 FROM decodex.read_reset_card_account_admission_exact($1::text::uuid,$2)";
const BIND_RESET_CARD_CREDIT_SQL: &str = "UPDATE decodex.outbox SET \
	 payload=jsonb_set(payload,'{reset_card_effect,provider_credit_id_hex}', \
	   to_jsonb($4::text),true) \
	 WHERE id=$1 AND aggregate_kind='reset_card_operation' AND state='in_flight' \
	 AND lease_holder=$2::text::uuid AND claim_token=$3::text::uuid \
	 AND lease_expires_at > clock_timestamp() AND effect_state='not_started' \
	 AND jsonb_typeof(payload->'reset_card_effect')='object' \
	 AND (payload->'reset_card_effect') ? 'provider_idempotency_key_hex' \
	 AND payload #>> '{reset_card_effect,credential_writer_operation_id}'=$5::text \
	 AND ((payload->'reset_card_effect') \
	   - 'provider_idempotency_key_hex' - 'provider_credit_id_hex' \
	   - 'credential_writer_operation_id')='{}'::jsonb \
	 AND (NOT ((payload->'reset_card_effect') ? 'provider_credit_id_hex') \
	   OR payload #>> '{reset_card_effect,provider_credit_id_hex}'=$4::text)";
const INSTALL_RESET_CARD_PRIVATE_EFFECT_SQL: &str = "UPDATE decodex.outbox SET \
	 payload=jsonb_set(payload,'{reset_card_effect}', \
	   jsonb_build_object('provider_idempotency_key_hex',$3::text, \
	     'credential_writer_operation_id',$4::text),true) \
	 WHERE aggregate_kind='reset_card_operation' AND aggregate_id=$1 \
	 AND aggregate_revision=$2 AND state='pending' \
	 AND NOT payload ? 'reset_card_effect'";
const RENEW_RESET_CARD_CLAIM_SQL: &str = "WITH write_time AS (SELECT clock_timestamp() AS value) \
	 UPDATE decodex.outbox \
	 SET lease_acquired_at=write_time.value, \
	   lease_expires_at=write_time.value + $4::bigint * interval '1 millisecond' \
	 FROM write_time \
	 WHERE id=$1 AND aggregate_kind='reset_card_operation' AND state='in_flight' \
	   AND lease_holder=$2::text::uuid AND claim_token=$3::text::uuid \
	   AND lease_expires_at > write_time.value";

/// Reserved non-Codex execution profile used by the direct account backend API path.
///
/// It is intentionally a fixed, non-secret marker.  It is not an executable hash and is never
/// used to authorize a Codex process callback.
pub const RESET_CARD_API_CALLBACK_PROFILE_SHA256: &str =
	"0000000000000000000000000000000000000000000000000000000000000000";

/// Durable public operation state returned after command preparation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResetCardPreparation {
	/// Exact selected vNext account.
	pub account_id: AccountId,
	/// Account revision fenced by preparation.
	pub account_revision: i64,
	/// Public card descriptor retained by the operation.
	pub descriptor: ResetCardDescriptor,
}

/// One fenced reset-card outbox claim.
///
/// Debug deliberately omits the exact provider identifier and provider idempotency key.
#[derive(Clone)]
pub struct ResetCardClaim {
	/// Outbox row identity.
	pub id: i64,
	/// Per-claim fencing token.
	claim_token: String,
	/// Exact selected vNext account.
	pub account_id: AccountId,
	/// Account revision fenced by preparation.
	pub account_revision: i64,
	/// Immutable credential/provider/callback snapshot admitted before the effect.
	pub process_binding: ProcessGenerationAccountBinding,
	/// Public card descriptor.
	pub descriptor: ResetCardDescriptor,
	/// Exact provider identifier, once durably bound before the effect fence.
	exact_credit_id: Option<String>,
	/// Exact provider retry key, durably equal to the logical command key.
	provider_idempotency_key: String,
	/// Whether an earlier owner crossed the external-effect fence.
	pub requires_reconciliation: bool,
	/// Previously recorded terminal outcome, when the effect receipt survived a crash.
	pub recorded_outcome: Option<ResetCardConsumeOutcome>,
}
impl ResetCardClaim {
	/// Borrow the private per-claim fencing token for a typed owner transition.
	pub fn claim_token(&self) -> &str {
		self.claim_token.as_str()
	}

	/// Borrow the exact provider credit identity when it was durably bound.
	pub fn exact_credit_id(&self) -> Option<&str> {
		self.exact_credit_id.as_deref()
	}

	/// Borrow the exact provider retry key without exposing it through [`Debug`].
	pub fn provider_idempotency_key(&self) -> &str {
		self.provider_idempotency_key.as_str()
	}
}
impl std::fmt::Debug for ResetCardClaim {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ResetCardClaim")
			.field("id", &self.id)
			.field("account_id", &self.account_id)
			.field("account_revision", &self.account_revision)
			.field("descriptor", &self.descriptor)
			.field("exact_credit_id_bound", &self.exact_credit_id.is_some())
			.field("requires_reconciliation", &self.requires_reconciliation)
			.field("receipt_recorded", &self.recorded_outcome.is_some())
			.finish()
	}
}

/// Closed durable reset-card operation projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetCardOperationStatus {
	/// No operation exists for the exact logical key.
	NotFound,
	/// The operation is prepared and no ambiguous external effect is recorded.
	Prepared,
	/// The external effect may have occurred and must not be blindly remapped or replayed.
	EffectAmbiguous,
	/// Receipt plus authoritative readback completed the operation.
	Completed(ResetCardConsumeOutcome),
	/// The operation terminally failed before the external-effect fence.
	FailedBeforeEffect(ResetCardFailureCode),
}

/// Credential-negative failure classifications persisted before an effect begins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetCardFailureCode {
	/// The selected account or revision stopped admitting manual use.
	AccountChanged,
	/// The selected host-vault entry was unavailable or rejected.
	VaultUnavailable,
	/// Compatibility failure retained for durable replay/decoding. The current direct provider
	/// API path does not inspect the installed executable build.
	SchemaUnsupported,
	/// Complete provider card details were unavailable or unsafe.
	InventoryIncomplete,
	/// The selected public descriptor no longer resolved uniquely.
	InventoryChanged,
	/// Provider process or protocol mechanics were unavailable.
	ProviderUnavailable,
	/// Bounded process capacity was unavailable.
	ResourceExhausted,
}
impl ResetCardFailureCode {
	/// Stable lower-snake-case database representation.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::AccountChanged => "reset_card_account_changed",
			Self::VaultUnavailable => "reset_card_vault_unavailable",
			Self::SchemaUnsupported => "reset_card_schema_unsupported",
			Self::InventoryIncomplete => "reset_card_inventory_incomplete",
			Self::InventoryChanged => "reset_card_inventory_changed",
			Self::ProviderUnavailable => "reset_card_provider_unavailable",
			Self::ResourceExhausted => "reset_card_resource_exhausted",
		}
	}

	fn parse(value: &str) -> Result<Self, StoreError> {
		match value {
			"reset_card_account_changed" => Ok(Self::AccountChanged),
			"reset_card_vault_unavailable" => Ok(Self::VaultUnavailable),
			"reset_card_schema_unsupported" => Ok(Self::SchemaUnsupported),
			"reset_card_inventory_incomplete" => Ok(Self::InventoryIncomplete),
			"reset_card_inventory_changed" => Ok(Self::InventoryChanged),
			"reset_card_provider_unavailable" => Ok(Self::ProviderUnavailable),
			"reset_card_resource_exhausted" => Ok(Self::ResourceExhausted),
			_ => Err(StoreError::Incompatible("stored reset-card failure code is unknown".into())),
		}
	}
}

impl PostgresStore {
	/// Atomically fence one exact account revision and enqueue a manual reset-card operation.
	///
	/// The public command response contains no exact provider credit identifier. A worker resolves
	/// and durably binds that identifier through [`Self::bind_reset_card_credit`] before it may
	/// cross the external-effect fence.
	pub async fn prepare_reset_card_operation(
		&self,
		command: &CommandIdentity,
		account_id: &AccountId,
		expected_revision: i64,
		process_binding: &ProcessGenerationAccountBinding,
		descriptor: ResetCardDescriptor,
	) -> Result<ResetCardPreparation, StoreError> {
		if expected_revision < 1 {
			return Err(StoreError::InvalidInput("expected revision must be positive"));
		}

		let mut client = self.pool().get().await?;
		let command_descriptor =
			reset_card_command_descriptor(account_id, expected_revision, descriptor);
		let reservation = match reserve_command(&mut client, command, &command_descriptor).await? {
			CommandClaim::Completed(response) => {
				let prepared = preparation_from_response(response)?;

				ensure_operation_exists(&client, &command.key).await?;

				return Ok(prepared);
			},
			CommandClaim::Owned(reservation) => reservation,
		};
		let transaction = match client.transaction().await {
			Ok(transaction) => transaction,
			Err(_) => return Err(StoreError::ResetCardCommitOutcomeUnknown),
		};
		match persist_reset_card_preparation(
			&transaction,
			&reservation,
			command,
			account_id,
			expected_revision,
			process_binding,
			descriptor,
		)
		.await
		{
			Ok(()) => {},
			Err(ResetCardPreparationPersistenceError::Rejected(rejection)) => {
				let _ = transaction.rollback().await;
				drop(client);
				let response = rejection_response(account_id, rejection);

				return if self.complete_reset_card_rejection(&reservation, &response).await.is_ok()
				{
					Err(rejection.store_error(account_id))
				} else {
					Err(StoreError::ResetCardCommitOutcomeUnknown)
				};
			},
			Err(ResetCardPreparationPersistenceError::Store(error)) => {
				let _ = transaction.rollback().await;

				return Err(error);
			},
		}
		if transaction.commit().await.is_err() {
			// Do not reuse the connection whose COMMIT acknowledgement was lost. A fresh
			// connection must observe the exact same logical key before the caller may classify
			// the command as rejected before acceptance.
			drop(client);

			return match classify_unknown_commit_readback(
				self.read_completed_reset_card_preparation(command, &command_descriptor).await,
			) {
				UnknownCommitReadback::Committed(prepared) => Ok(prepared),
				UnknownCommitReadback::Unresolved => Err(StoreError::ResetCardCommitOutcomeUnknown),
			};
		}

		Ok(ResetCardPreparation {
			account_id: account_id.clone(),
			account_revision: expected_revision,
			descriptor,
		})
	}

	/// Atomically prepare one reset-card operation for the direct backend API path.
	///
	/// The existing outbox format is retained for durable upgrade compatibility, but its execution
	/// profile is a reserved API marker rather than a Codex executable/callback capability.
	pub async fn prepare_reset_card_api_operation(
		&self,
		command: &CommandIdentity,
		account_id: &AccountId,
		expected_revision: i64,
		credential_binding: &CredentialBinding,
		descriptor: ResetCardDescriptor,
	) -> Result<ResetCardPreparation, StoreError> {
		let process_binding = ProcessGenerationAccountBinding::new(
			expected_revision,
			credential_binding.clone(),
			RESET_CARD_API_CALLBACK_PROFILE_SHA256.to_owned(),
		)
		.map_err(|_| StoreError::InvalidInput("direct API reset-card binding is invalid"))?;
		self.prepare_reset_card_operation(
			command,
			account_id,
			expected_revision,
			&process_binding,
			descriptor,
		)
		.await
	}

	/// Replay one completed exact preparation without applying current account or vault admission.
	///
	/// Callers use this read before new-command gates. A completed receipt remains authoritative
	/// when account state, revision, or host-vault configuration changed after acceptance.
	pub async fn replay_reset_card_preparation(
		&self,
		command: &CommandIdentity,
		account_id: &AccountId,
		expected_revision: i64,
		descriptor: ResetCardDescriptor,
		callback_profile_sha256: Option<&str>,
	) -> Result<Option<ResetCardPreparation>, StoreError> {
		if expected_revision < 1 {
			return Err(StoreError::InvalidInput("expected revision must be positive"));
		}
		let command_descriptor =
			reset_card_command_descriptor(account_id, expected_revision, descriptor);

		match self
			.inspect_reset_card_preparation(command, &command_descriptor)
			.await
			.map_err(pending_replay_receipt_error)?
		{
			ResetCardPreparationReplay::Absent => Ok(None),
			ResetCardPreparationReplay::Pending { claim_expired: false } =>
				Err(StoreError::ResetCardCommitOutcomeUnknown),
			ResetCardPreparationReplay::Pending { claim_expired: true } => {
				let callback_profile_sha256 = callback_profile_sha256
					.filter(|value| valid_sha256(value))
					.ok_or(StoreError::ResetCardCommitOutcomeUnknown)?;
				// This first account observation never overlaps a receipt reservation. It lets
				// clear continuation cases obtain their real host-store binding without renewing
				// the expired command claim.
				match self
					.observe_pending_reset_card_recovery(
						account_id,
						expected_revision,
						callback_profile_sha256,
					)
					.await
					.map_err(|_| StoreError::ResetCardCommitOutcomeUnknown)?
				{
					PendingResetCardRecovery::Continue => {
						return Ok(None);
					},
					PendingResetCardRecovery::StoreUnavailable => {
						return Err(StoreError::ResetCardCommitOutcomeUnknown);
					},
					PendingResetCardRecovery::Reject(_) => {},
				}
				let mut client = self
					.pool()
					.get()
					.await
					.map_err(|_| StoreError::ResetCardCommitOutcomeUnknown)?;

				match reserve_command(&mut client, command, &command_descriptor)
					.await
					.map_err(pending_replay_receipt_error)?
				{
					CommandClaim::Owned(reservation) => {
						// Receipt ownership is established before the account lifecycle lock. The
						// same transaction revalidates account admission and completes only a
						// rejection that remains deterministic under that canonical lock order.
						let transaction = client
							.transaction()
							.await
							.map_err(|_| StoreError::ResetCardCommitOutcomeUnknown)?;
						if transaction
							.query_one(ACCOUNT_LIFECYCLE_LOCK_SQL, &[&account_id.as_str()])
							.await
							.is_err()
						{
							let _ = transaction.rollback().await;

							return Err(StoreError::ResetCardCommitOutcomeUnknown);
						}
						let account = match transaction
							.query_opt(
								RESET_CARD_ACCOUNT_ADMISSION_SQL,
								&[&account_id.as_str(), &callback_profile_sha256],
							)
							.await
						{
							Ok(account) => account,
							Err(_) => {
								let _ = transaction.rollback().await;

								return Err(StoreError::ResetCardCommitOutcomeUnknown);
							},
						};
						let rejection = match classify_pending_reset_card_recovery(
							account.as_ref(),
							expected_revision,
						) {
							Ok(PendingResetCardRecovery::Reject(rejection)) => rejection,
							Ok(
								PendingResetCardRecovery::Continue
								| PendingResetCardRecovery::StoreUnavailable,
							)
							| Err(_) => {
								let _ = transaction.rollback().await;

								return Err(StoreError::ResetCardCommitOutcomeUnknown);
							},
						};
						let response = rejection_response(account_id, rejection);
						if finish_command(&transaction, &reservation, &response).await.is_err() {
							let _ = transaction.rollback().await;

							return Err(StoreError::ResetCardCommitOutcomeUnknown);
						}
						if transaction.commit().await.is_err() {
							return Err(StoreError::ResetCardCommitOutcomeUnknown);
						}

						Err(rejection.store_error(account_id))
					},
					CommandClaim::Completed(response) => {
						let prepared = preparation_from_response(response)?;

						ensure_operation_exists(&client, &command.key).await?;

						Ok(Some(prepared))
					},
				}
			},
			ResetCardPreparationReplay::Completed(prepared) => Ok(Some(prepared)),
		}
	}

	/// Replay a direct backend API reset-card preparation with the reserved API execution marker.
	pub async fn replay_reset_card_api_preparation(
		&self,
		command: &CommandIdentity,
		account_id: &AccountId,
		expected_revision: i64,
		descriptor: ResetCardDescriptor,
	) -> Result<Option<ResetCardPreparation>, StoreError> {
		self.replay_reset_card_preparation(
			command,
			account_id,
			expected_revision,
			descriptor,
			Some(RESET_CARD_API_CALLBACK_PROFILE_SHA256),
		)
		.await
	}

	async fn observe_pending_reset_card_recovery(
		&self,
		account_id: &AccountId,
		expected_revision: i64,
		callback_profile_sha256: &str,
	) -> Result<PendingResetCardRecovery, StoreError> {
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;

		transaction.query_one(ACCOUNT_LIFECYCLE_LOCK_SQL, &[&account_id.as_str()]).await?;
		let account = transaction
			.query_opt(
				RESET_CARD_ACCOUNT_ADMISSION_SQL,
				&[&account_id.as_str(), &callback_profile_sha256],
			)
			.await?;
		let recovery = classify_pending_reset_card_recovery(account.as_ref(), expected_revision)?;

		transaction.rollback().await?;

		Ok(recovery)
	}

	async fn inspect_reset_card_preparation(
		&self,
		command: &CommandIdentity,
		descriptor: &CommandDescriptor,
	) -> Result<ResetCardPreparationReplay, StoreError> {
		let client = self.pool().get().await?;
		let response = match inspect_command_receipt(&client, command, descriptor).await? {
			CommandReplay::Absent => return Ok(ResetCardPreparationReplay::Absent),
			CommandReplay::Pending { claim_expired } =>
				return Ok(ResetCardPreparationReplay::Pending { claim_expired }),
			CommandReplay::Completed(response) => response,
		};
		let prepared = preparation_from_response(response)?;

		ensure_operation_exists(&client, &command.key).await?;

		Ok(ResetCardPreparationReplay::Completed(prepared))
	}

	async fn read_completed_reset_card_preparation(
		&self,
		command: &CommandIdentity,
		descriptor: &CommandDescriptor,
	) -> Result<Option<ResetCardPreparation>, StoreError> {
		match self.inspect_reset_card_preparation(command, descriptor).await? {
			ResetCardPreparationReplay::Completed(prepared) => Ok(Some(prepared)),
			ResetCardPreparationReplay::Absent | ResetCardPreparationReplay::Pending { .. } =>
				Ok(None),
		}
	}

	async fn complete_reset_card_rejection(
		&self,
		reservation: &CommandReservation,
		response: &Value,
	) -> Result<(), StoreError> {
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;

		finish_command(&transaction, reservation, response).await?;
		transaction.commit().await?;

		Ok(())
	}

	/// Claim at most one reset-card operation without consuming unrelated outbox work.
	pub async fn claim_reset_card_operation(
		&self,
		worker_id: &str,
		lease: Duration,
	) -> Result<Option<ResetCardClaim>, StoreError> {
		let lease_millis = crate::exact_milliseconds(lease)?;
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;
		let row = transaction
			.query_opt(
				"WITH write_time AS MATERIALIZED (SELECT clock_timestamp() AS value), \
				 exhausted AS ( \
				   UPDATE decodex.outbox SET state='dead_letter', \
				     payload=payload - 'reset_card_effect', lease_holder=NULL, \
				     claim_token=NULL, lease_acquired_at=NULL, lease_expires_at=NULL, \
				     dead_lettered_at=write_time.value, \
				     last_failure_code=COALESCE(last_failure_code,'reset_card_provider_unavailable') \
				   FROM write_time \
				   WHERE aggregate_kind='reset_card_operation' AND state='in_flight' \
				     AND lease_expires_at <= write_time.value AND attempt_count >= max_attempts \
				     AND effect_state='not_started' RETURNING id \
				 ), candidate AS ( \
				   SELECT id, effect_state <> 'not_started' AS requires_reconciliation \
				   FROM decodex.outbox CROSS JOIN write_time \
				   WHERE aggregate_kind='reset_card_operation' \
				     AND available_at <= write_time.value \
				     AND (attempt_count < max_attempts OR effect_state <> 'not_started') \
				     AND (state='pending' OR (state='in_flight' AND lease_expires_at <= write_time.value)) \
				   ORDER BY available_at,id FOR UPDATE SKIP LOCKED LIMIT 1 \
				 ) \
				 UPDATE decodex.outbox AS work SET state='in_flight', \
				   attempt_count=CASE WHEN work.attempt_count < work.max_attempts \
				     THEN work.attempt_count + 1 ELSE work.attempt_count END, \
				   lease_holder=$1::text::uuid, claim_token=gen_random_uuid(), \
				   lease_acquired_at=write_time.value, \
				   lease_expires_at=write_time.value + $2::bigint * interval '1 millisecond' \
				 FROM candidate CROSS JOIN write_time WHERE work.id=candidate.id \
				 RETURNING work.id,work.claim_token::text, \
				   work.payload - 'reset_card_effect',work.payload->'reset_card_effect', \
				   candidate.requires_reconciliation,work.receipt",
				&[&worker_id, &lease_millis],
			)
			.await?;

		transaction.commit().await?;

		row.map(reset_card_claim).transpose()
	}

	/// Renew only the live typed reset-card claim identified by its worker and fencing token.
	pub async fn renew_reset_card_claim(
		&self,
		id: i64,
		worker_id: &str,
		claim_token: &str,
		lease: Duration,
	) -> Result<(), StoreError> {
		let lease_millis = crate::exact_milliseconds(lease)?;
		let updated = self
			.pool()
			.get()
			.await?
			.execute(RENEW_RESET_CARD_CLAIM_SQL, &[&id, &worker_id, &claim_token, &lease_millis])
			.await?;

		if updated == 1 { Ok(()) } else { Err(StoreError::OwnershipLost("reset-card claim")) }
	}

	/// Durably bind the exact provider credit before the external-effect fence.
	pub async fn bind_reset_card_credit(
		&self,
		claim: &ResetCardClaim,
		worker_id: &str,
		exact_credit_id: &str,
	) -> Result<(), StoreError> {
		if !valid_exact_credit_id(exact_credit_id) {
			return Err(StoreError::InvalidInput("exact reset-card credit identity is invalid"));
		}
		if let Some(bound) = claim.exact_credit_id()
			&& bound != exact_credit_id
		{
			return Err(StoreError::Incompatible(
				"reset-card operation attempted to remap its exact credit".into(),
			));
		}
		let encoded_credit_id = encode_private_text(exact_credit_id);

		let updated = self
			.pool()
			.get()
			.await?
			.execute(
				BIND_RESET_CARD_CREDIT_SQL,
				&[
					&claim.id,
					&worker_id,
					&claim.claim_token,
					&encoded_credit_id,
					&claim.process_binding.credential.writer_operation_id.as_str(),
				],
			)
			.await?;

		if updated == 1 { Ok(()) } else { Err(StoreError::OwnershipLost("reset-card claim")) }
	}

	/// Atomically recheck the retained provider/store binding and cross the reset-card effect
	/// fence. Administrative revision changes do not revoke already-accepted work.
	///
	/// A public-selection advisory lock and oldest-operation check prevent two logical keys from
	/// concurrently consuming the same card. The account share lock keeps its exact revision and
	/// admission state stable until the effect fence commits.
	pub async fn begin_reset_card_effect(
		&self,
		claim: &ResetCardClaim,
		worker_id: &str,
	) -> Result<(), StoreError> {
		let selection_lock = format!(
			"decodex/reset-card-selection/1/{}/{}/{}",
			claim.account_id.as_str(),
			claim.descriptor.granted_at().unix_seconds(),
			claim.descriptor.expires_at().unix_seconds(),
		);
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;

		transaction
			.query_one(
				"SELECT pg_catalog.pg_advisory_xact_lock( \
				   pg_catalog.hashtextextended($1,0))",
				&[&selection_lock],
			)
			.await?;
		transaction.query_one(ACCOUNT_LIFECYCLE_LOCK_SQL, &[&claim.account_id.as_str()]).await?;
		let oldest: i64 = transaction
			.query_one(
				"SELECT id FROM decodex.outbox \
				 WHERE aggregate_kind='reset_card_operation' \
				   AND payload #>> '{payload,account_id}'=$1 \
				   AND payload #> '{payload,granted_at_unix_seconds}'=to_jsonb($2::bigint) \
				   AND payload #> '{payload,expires_at_unix_seconds}'=to_jsonb($3::bigint) \
				   AND (state IN ('pending','in_flight') OR (effect_state <> 'not_started' \
				     AND NOT (state='delivered' AND effect_state='receipt_recorded' \
				       AND receipt @> '{\"outcome\":\"nothing_to_reset\"}'::jsonb \
				       AND reconciliation @> \
				         '{\"schema\":\"decodex/reset-card-readback/1\", \
				            \"outcome\":\"nothing_to_reset\", \
				            \"selected_exact_credit_available\":true}'::jsonb \
				       AND reconciliation->>'account_id'=payload #>> '{payload,account_id}' \
				       AND reconciliation->'account_revision'= \
				         payload #> '{payload,account_revision}'))) \
				 ORDER BY id LIMIT 1",
				&[
					&claim.account_id.as_str(),
					&claim.descriptor.granted_at().unix_seconds(),
					&claim.descriptor.expires_at().unix_seconds(),
				],
			)
			.await?
			.get(0);
		if oldest != claim.id {
			return Err(StoreError::ResetCardSelectionConflict);
		}

		let account = transaction
			.query_opt(
				RESET_CARD_ACCOUNT_ADMISSION_SQL,
				&[
					&claim.account_id.as_str(),
					&claim.process_binding.refresh_callback_profile_sha256,
				],
			)
			.await?
			.ok_or(StoreError::InvalidInput("reset-card account is not enrolled"))?;
		let state =
			account_state(account.get(0)).map_err(|_| StoreError::ResetCardCommitOutcomeUnknown)?;
		match reset_card_account_admission(&account, state)
			.map_err(|_| StoreError::ResetCardCommitOutcomeUnknown)?
		{
			ResetCardAccountAdmission::Admitted
				if row_matches_process_binding(&account, &claim.process_binding) => {},
			ResetCardAccountAdmission::StoreUnavailable =>
				return Err(StoreError::ResetCardCommitOutcomeUnknown),
			ResetCardAccountAdmission::Admitted | ResetCardAccountAdmission::Rejected =>
				return Err(StoreError::InvalidInput(
					"account lifecycle rejects manual reset-card use",
				)),
		}

		let updated = transaction
			.execute(
				"UPDATE decodex.outbox SET effect_state='ambiguous' \
				 WHERE id=$1 AND aggregate_kind='reset_card_operation' AND state='in_flight' \
				   AND lease_holder=$2::text::uuid AND claim_token=$3::text::uuid \
				   AND lease_expires_at > clock_timestamp() AND effect_state='not_started' \
				   AND payload #>> '{reset_card_effect,provider_credit_id_hex}' IS NOT NULL",
				&[&claim.id, &worker_id, &claim.claim_token],
			)
			.await?;
		if updated != 1 {
			return Err(StoreError::OwnershipLost("reset-card claim"));
		}

		transaction.commit().await?;

		Ok(())
	}

	/// Terminally reject an operation while the external-effect fence is still untouched.
	pub async fn fail_reset_card_before_effect(
		&self,
		claim: &ResetCardClaim,
		worker_id: &str,
		failure: ResetCardFailureCode,
	) -> Result<(), StoreError> {
		let updated = self
			.pool()
			.get()
			.await?
			.execute(
				"WITH write_time AS (SELECT clock_timestamp() AS value) \
				 UPDATE decodex.outbox SET state='dead_letter',attempt_count=max_attempts, \
				   payload=payload - 'reset_card_effect', \
				   last_failure_code=$4,dead_lettered_at=write_time.value, \
				   lease_holder=NULL,claim_token=NULL,lease_acquired_at=NULL,lease_expires_at=NULL \
				 FROM write_time WHERE id=$1 AND aggregate_kind='reset_card_operation' \
				   AND state='in_flight' AND lease_holder=$2::text::uuid \
				   AND claim_token=$3::text::uuid AND lease_expires_at > write_time.value \
				   AND effect_state='not_started'",
				&[&claim.id, &worker_id, &claim.claim_token, &failure.as_str()],
			)
			.await?;

		if updated == 1 { Ok(()) } else { Err(StoreError::OwnershipLost("reset-card claim")) }
	}

	/// Observe one durable operation by its exact idempotency key.
	pub async fn reset_card_operation_status(
		&self,
		idempotency_key: &str,
	) -> Result<ResetCardOperationStatus, StoreError> {
		if idempotency_key.is_empty() || idempotency_key.len() > 256 {
			return Err(StoreError::InvalidInput("idempotency key must contain 1..=256 bytes"));
		}
		crate::ensure_credential_negative_text(idempotency_key)?;
		let aggregate_id = operation_aggregate_id(idempotency_key);
		let row = self
			.pool()
			.get()
			.await?
			.query_opt(
				"SELECT state::text,effect_state::text,receipt,reconciliation,last_failure_code \
				 FROM decodex.outbox WHERE aggregate_kind='reset_card_operation' \
				 AND aggregate_id=$1",
				&[&aggregate_id],
			)
			.await?;

		row.map(operation_status)
			.transpose()
			.map(|status| status.unwrap_or(ResetCardOperationStatus::NotFound))
	}

	/// Report whether an account has reset-card work whose provider identity is still relevant.
	///
	/// A host may initialize a previously absent durable provider binding only when this returns
	/// false. Completed operations and failures proved to precede the effect fence are safe;
	/// prepared, ambiguous, and receipt-only rows are not.
	pub async fn reset_card_account_has_unsettled_operations(
		&self,
		account_id: &AccountId,
	) -> Result<bool, StoreError> {
		let unsettled = self
			.pool()
			.get()
			.await?
			.query_one(
				"SELECT EXISTS (SELECT 1 FROM decodex.outbox \
				 WHERE aggregate_kind='reset_card_operation' \
				   AND payload #>> '{payload,account_id}'=$1 \
				   AND NOT ((state='delivered' AND effect_state='receipt_recorded') \
				     OR (state='dead_letter' AND effect_state='not_started')))",
				&[&account_id.as_str()],
			)
			.await?
			.get(0);

		Ok(unsettled)
	}
}

#[cfg(all(test, feature = "test-support"))]
pub(crate) async fn prepare_account_bound_reset_card_sql(
	client: &tokio_postgres::Client,
) -> Result<usize, StoreError> {
	const SOURCES: [&str; 4] = [
		ACCOUNT_LIFECYCLE_LOCK_SQL,
		RESET_CARD_ACCOUNT_ADMISSION_SQL,
		BIND_RESET_CARD_CREDIT_SQL,
		INSTALL_RESET_CARD_PRIVATE_EFFECT_SQL,
	];
	for source in SOURCES {
		client.prepare(source).await?;
	}
	Ok(SOURCES.len())
}

enum ResetCardPreparationReplay {
	Absent,
	Pending { claim_expired: bool },
	Completed(ResetCardPreparation),
}

enum ResetCardPreparationPersistenceError {
	Rejected(ResetCardPreparationRejection),
	Store(StoreError),
}
impl From<StoreError> for ResetCardPreparationPersistenceError {
	fn from(error: StoreError) -> Self {
		Self::Store(error)
	}
}
impl From<tokio_postgres::Error> for ResetCardPreparationPersistenceError {
	fn from(error: tokio_postgres::Error) -> Self {
		Self::Store(error.into())
	}
}

#[derive(Clone, Copy)]
enum ResetCardPreparationRejection {
	NotEnrolled,
	RevisionChanged { expected: i64, actual: i64 },
	StateRejected,
}

enum PendingResetCardRecovery {
	Continue,
	StoreUnavailable,
	Reject(ResetCardPreparationRejection),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResetCardAccountAdmission {
	Admitted,
	StoreUnavailable,
	Rejected,
}
impl ResetCardPreparationRejection {
	fn store_error(self, account_id: &AccountId) -> StoreError {
		match self {
			Self::NotEnrolled => StoreError::InvalidInput("reset-card account is not enrolled"),
			Self::RevisionChanged { expected, actual } => StoreError::RevisionConflict {
				entity: format!("account/{account_id}"),
				expected: Some(expected),
				actual: Some(actual),
			},
			Self::StateRejected =>
				StoreError::InvalidInput("account state rejects manual reset-card use"),
		}
	}
}

async fn persist_reset_card_preparation(
	transaction: &deadpool_postgres::Transaction<'_>,
	reservation: &CommandReservation,
	command: &CommandIdentity,
	account_id: &AccountId,
	expected_revision: i64,
	process_binding: &ProcessGenerationAccountBinding,
	descriptor: ResetCardDescriptor,
) -> Result<(), ResetCardPreparationPersistenceError> {
	transaction.query_one(ACCOUNT_LIFECYCLE_LOCK_SQL, &[&account_id.as_str()]).await?;
	let account = transaction
		.query_opt(
			RESET_CARD_ACCOUNT_ADMISSION_SQL,
			&[&account_id.as_str(), &process_binding.refresh_callback_profile_sha256],
		)
		.await?
		.ok_or(ResetCardPreparationPersistenceError::Rejected(
			ResetCardPreparationRejection::NotEnrolled,
		))?;
	let state = account_state(account.get(0)).map_err(|_| {
		ResetCardPreparationPersistenceError::Store(StoreError::ResetCardCommitOutcomeUnknown)
	})?;
	let actual_revision: i64 = account.get(1);

	if actual_revision != expected_revision {
		return Err(ResetCardPreparationPersistenceError::Rejected(
			ResetCardPreparationRejection::RevisionChanged {
				expected: expected_revision,
				actual: actual_revision,
			},
		));
	}
	let admission = reset_card_account_admission(&account, state).map_err(|_| {
		ResetCardPreparationPersistenceError::Store(StoreError::ResetCardCommitOutcomeUnknown)
	})?;
	match admission {
		ResetCardAccountAdmission::Admitted
			if process_binding.account_revision == expected_revision
				&& row_matches_process_binding(&account, process_binding) => {},
		ResetCardAccountAdmission::StoreUnavailable =>
			return Err(ResetCardPreparationPersistenceError::Store(
				StoreError::ResetCardCommitOutcomeUnknown,
			)),
		ResetCardAccountAdmission::Admitted | ResetCardAccountAdmission::Rejected =>
			return Err(ResetCardPreparationPersistenceError::Rejected(
				ResetCardPreparationRejection::StateRejected,
			)),
	}

	let public_payload =
		operation_public_payload(account_id, expected_revision, process_binding, descriptor);
	let aggregate_id = operation_aggregate_id(&command.key);

	append_activity_and_outbox(
		transaction,
		AGGREGATE_KIND,
		&aggregate_id,
		1,
		EVENT_KIND,
		&aggregate_id,
		&public_payload,
	)
	.await?;
	let encoded_provider_key = encode_private_text(&command.key);
	let writer_operation_id = process_binding.credential.writer_operation_id.as_str();
	let private_projection_updated = transaction
		.execute(
			INSTALL_RESET_CARD_PRIVATE_EFFECT_SQL,
			&[&aggregate_id, &1_i64, &encoded_provider_key, &writer_operation_id],
		)
		.await?;
	if private_projection_updated != 1 {
		return Err(ResetCardPreparationPersistenceError::Store(incompatible_error(
			"prepared reset-card operation did not receive one private provider key",
		)));
	}

	let response = preparation_response(account_id, expected_revision, descriptor);

	finish_command(transaction, reservation, &response).await?;

	Ok(())
}

fn classify_pending_reset_card_recovery(
	account: Option<&Row>,
	expected_revision: i64,
) -> Result<PendingResetCardRecovery, StoreError> {
	let Some(account) = account else {
		return Ok(PendingResetCardRecovery::Continue);
	};
	let actual_revision: i64 = account.get(1);
	if actual_revision < 1 {
		return incompatible("stored reset-card account revision is invalid");
	}
	if actual_revision > expected_revision {
		return Ok(PendingResetCardRecovery::Reject(
			ResetCardPreparationRejection::RevisionChanged {
				expected: expected_revision,
				actual: actual_revision,
			},
		));
	}
	if actual_revision < expected_revision {
		return Ok(PendingResetCardRecovery::Continue);
	}

	match reset_card_account_admission(account, account_state(account.get(0))?)? {
		ResetCardAccountAdmission::Admitted => Ok(PendingResetCardRecovery::Continue),
		ResetCardAccountAdmission::StoreUnavailable =>
			Ok(PendingResetCardRecovery::StoreUnavailable),
		ResetCardAccountAdmission::Rejected =>
			Ok(PendingResetCardRecovery::Reject(ResetCardPreparationRejection::StateRejected)),
	}
}

fn pending_replay_receipt_error(error: StoreError) -> StoreError {
	match error {
		StoreError::IdempotencyConflict
		| StoreError::Incompatible(_)
		| StoreError::RevisionConflict { .. }
		| StoreError::InvalidInput(
			"reset-card account is not enrolled" | "account state rejects manual reset-card use",
		) => error,
		_ => StoreError::ResetCardCommitOutcomeUnknown,
	}
}

fn reset_card_account_admission(
	account: &Row,
	state: AccountState,
) -> Result<ResetCardAccountAdmission, StoreError> {
	let store_observation = account.get::<_, &str>(10);
	if !matches!(
		store_observation,
		"unknown" | "exact" | "missing" | "mismatch" | "provider_mismatch" | "unavailable"
	) {
		return incompatible("stored reset-card account observation is invalid");
	}
	let credential_complete = stored_reset_card_credential_is_complete(account)?;
	if store_observation == "unavailable" {
		return Ok(ResetCardAccountAdmission::StoreUnavailable);
	}
	if admit_manual_reset_card_use(state).is_err()
		|| !account.get::<_, bool>(2)
		|| account.get::<_, bool>(3)
		|| !credential_complete
		|| account.get::<_, bool>(11)
		|| !account.get::<_, bool>(12)
	{
		return Ok(ResetCardAccountAdmission::Rejected);
	}

	Ok(match store_observation {
		"exact" => ResetCardAccountAdmission::Admitted,
		_ => ResetCardAccountAdmission::Rejected,
	})
}

fn stored_reset_card_credential_is_complete(account: &Row) -> Result<bool, StoreError> {
	let schema_version = account.get::<_, Option<i32>>(4);
	let credential_version = account.get::<_, Option<i64>>(5);
	let fingerprint = account.get::<_, Option<&str>>(6);
	let writer_operation_id = account.get::<_, Option<&str>>(7);
	let provider_kind = account.get::<_, Option<&str>>(8);
	let provider_account_id = account.get::<_, Option<&str>>(9);
	let fields_present = [
		schema_version.is_some(),
		credential_version.is_some(),
		fingerprint.is_some(),
		writer_operation_id.is_some(),
		provider_kind.is_some(),
		provider_account_id.is_some(),
	];

	if fields_present.iter().all(|present| !present) {
		return Ok(false);
	}
	if !fields_present.iter().all(|present| *present) {
		return incompatible("stored reset-card credential binding is incomplete");
	}
	let schema_version = u16::try_from(schema_version.expect("presence checked"))
		.ok()
		.and_then(|value| CredentialStoreSchemaVersion::new(value).ok());
	let credential_version = u64::try_from(credential_version.expect("presence checked"))
		.ok()
		.and_then(|value| CredentialVersion::new(value).ok());
	let fingerprint = CredentialFingerprint::new(fingerprint.expect("presence checked"));
	let writer_operation_id =
		AccountOperationId::new(writer_operation_id.expect("presence checked"));
	let provider = if provider_kind == Some(provider_text(AccountProvider::Chatgpt)) {
		ProviderIdentity::new(
			AccountProvider::Chatgpt,
			provider_account_id.expect("presence checked"),
		)
		.ok()
	} else {
		None
	};
	if schema_version.is_none()
		|| credential_version.is_none()
		|| fingerprint.is_err()
		|| writer_operation_id.is_err()
		|| provider.is_none()
	{
		return incompatible("stored reset-card credential binding is invalid");
	}

	Ok(true)
}

fn valid_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn row_matches_process_binding(row: &Row, binding: &ProcessGenerationAccountBinding) -> bool {
	let credential = &binding.credential;
	let Ok(version) = i64::try_from(credential.version.get()) else { return false };
	row.get::<_, Option<i32>>(4) == Some(i32::from(credential.schema_version.get()))
		&& row.get::<_, Option<i64>>(5) == Some(version)
		&& row.get::<_, Option<&str>>(6) == Some(credential.fingerprint.as_str())
		&& row.get::<_, Option<&str>>(7) == Some(credential.writer_operation_id.as_str())
		&& row.get::<_, Option<&str>>(8) == Some(provider_text(credential.provider.provider()))
		&& row.get::<_, Option<&str>>(9) == Some(credential.provider.account_id())
}

enum UnknownCommitReadback<T> {
	Committed(T),
	Unresolved,
}

fn classify_unknown_commit_readback<T>(
	readback: Result<Option<T>, StoreError>,
) -> UnknownCommitReadback<T> {
	match readback {
		Ok(Some(value)) => UnknownCommitReadback::Committed(value),
		// A fresh absence can race the original backend while it still completes COMMIT.
		Ok(None) | Err(_) => UnknownCommitReadback::Unresolved,
	}
}

fn reset_card_claim(row: Row) -> Result<ResetCardClaim, StoreError> {
	let payload: Value = row.get(2);
	if payload.get(PRIVATE_EFFECT_FIELD).is_some() {
		return incompatible("stored reset-card private effect material was not isolated");
	}
	let operation = payload
		.get("payload")
		.and_then(Value::as_object)
		.ok_or_else(|| incompatible_error("stored reset-card outbox payload is malformed"))?;
	if operation.get("schema").and_then(Value::as_str) != Some(PROTOCOL) {
		return incompatible("stored reset-card outbox schema is incompatible");
	}
	let account_id = AccountId::new(required_str(operation, "account_id")?)
		.map_err(|_| incompatible_error("stored reset-card account identity is invalid"))?;
	let account_revision = required_i64(operation, "account_revision")?;
	if account_revision < 1 {
		return incompatible("stored reset-card account revision is invalid");
	}
	let descriptor = stored_descriptor(operation)?;
	let private_effect: Value = row.get(3);
	let private_effect = private_effect.as_object().ok_or_else(|| {
		incompatible_error("stored reset-card private effect material is malformed")
	})?;
	let writer_operation_id = required_str(private_effect, PRIVATE_WRITER_OPERATION_ID_FIELD)?;
	let process_binding =
		process_binding_from_operation(operation, account_revision, writer_operation_id)?;
	let provider_idempotency_key = decode_private_text(
		required_str(private_effect, PRIVATE_PROVIDER_KEY_FIELD)?,
		MAX_PRIVATE_PROVIDER_KEY_BYTES,
		"stored reset-card provider key is malformed",
	)?;
	let exact_credit_id = private_effect
		.get(PRIVATE_CREDIT_ID_FIELD)
		.map(|encoded| {
			encoded
				.as_str()
				.ok_or_else(|| {
					incompatible_error("stored reset-card exact credit binding is invalid")
				})
				.and_then(decode_exact_credit_id)
		})
		.transpose()?;
	let expected_private_fields = usize::from(exact_credit_id.is_some()) + 2;
	if private_effect.len() != expected_private_fields {
		return incompatible("stored reset-card private effect material has unknown fields");
	}
	let requires_reconciliation: bool = row.get(4);
	let recorded_outcome =
		row.get::<_, Option<Value>>(5).as_ref().map(outcome_from_receipt).transpose()?;
	if !requires_reconciliation && recorded_outcome.is_some() {
		return incompatible("stored reset-card receipt precedes the effect fence");
	}

	Ok(ResetCardClaim {
		id: row.get(0),
		claim_token: row.get(1),
		account_id,
		account_revision,
		process_binding,
		descriptor,
		exact_credit_id,
		provider_idempotency_key,
		requires_reconciliation,
		recorded_outcome,
	})
}

fn operation_status(row: Row) -> Result<ResetCardOperationStatus, StoreError> {
	let state: &str = row.get(0);
	let effect_state: &str = row.get(1);
	let receipt: Option<Value> = row.get(2);
	let reconciliation: Option<Value> = row.get(3);
	let failure: Option<&str> = row.get(4);

	match (state, effect_state) {
		("delivered", "receipt_recorded") => {
			if reconciliation.is_none() {
				return incompatible("delivered reset-card operation lost readback");
			}
			let receipt =
				receipt.ok_or_else(|| incompatible_error("reset-card receipt is absent"))?;

			Ok(ResetCardOperationStatus::Completed(outcome_from_receipt(&receipt)?))
		},
		("dead_letter", "not_started") =>
			Ok(ResetCardOperationStatus::FailedBeforeEffect(ResetCardFailureCode::parse(
				failure.ok_or_else(|| incompatible_error("reset-card failure code is absent"))?,
			)?)),
		("pending" | "in_flight", "not_started") => Ok(ResetCardOperationStatus::Prepared),
		("pending" | "in_flight" | "dead_letter", "ambiguous" | "receipt_recorded") =>
			Ok(ResetCardOperationStatus::EffectAmbiguous),
		_ => incompatible("stored reset-card operation state is incoherent"),
	}
}

fn outcome_from_receipt(receipt: &Value) -> Result<ResetCardConsumeOutcome, StoreError> {
	match receipt.get("outcome").and_then(Value::as_str) {
		Some("reset") => Ok(ResetCardConsumeOutcome::Reset),
		Some("nothing_to_reset") => Ok(ResetCardConsumeOutcome::NothingToReset),
		Some("no_credit") => Ok(ResetCardConsumeOutcome::NoCredit),
		Some("already_redeemed") => Ok(ResetCardConsumeOutcome::AlreadyRedeemed),
		_ => incompatible("stored reset-card receipt outcome is invalid"),
	}
}

fn operation_public_payload(
	account_id: &AccountId,
	account_revision: i64,
	process_binding: &ProcessGenerationAccountBinding,
	descriptor: ResetCardDescriptor,
) -> Value {
	let credential = &process_binding.credential;
	json!({
		"schema": PROTOCOL,
		"account_id": account_id.as_str(),
		"account_revision": account_revision,
		"credential_store_schema_version": credential.schema_version.get(),
		"credential_version": credential.version.get(),
		"credential_fingerprint": credential.fingerprint.as_str(),
		"provider_kind": provider_text(credential.provider.provider()),
		"provider_account_id": credential.provider.account_id(),
		"refresh_callback_profile_sha256": process_binding.refresh_callback_profile_sha256,
		"granted_at_unix_seconds": descriptor.granted_at().unix_seconds(),
		"expires_at_unix_seconds": descriptor.expires_at().unix_seconds(),
	})
}

fn process_binding_from_operation(
	operation: &serde_json::Map<String, Value>,
	account_revision: i64,
	writer_operation_id: &str,
) -> Result<ProcessGenerationAccountBinding, StoreError> {
	let schema = u16::try_from(required_i64(operation, "credential_store_schema_version")?)
		.ok()
		.and_then(|value| CredentialStoreSchemaVersion::new(value).ok())
		.ok_or_else(|| incompatible_error("stored reset-card credential schema is invalid"))?;
	let version = u64::try_from(required_i64(operation, "credential_version")?)
		.ok()
		.and_then(|value| CredentialVersion::new(value).ok())
		.ok_or_else(|| incompatible_error("stored reset-card credential version is invalid"))?;
	let fingerprint =
		CredentialFingerprint::new(required_str(operation, "credential_fingerprint")?).map_err(
			|_| incompatible_error("stored reset-card credential fingerprint is invalid"),
		)?;
	let writer_operation_id = AccountOperationId::new(writer_operation_id)
		.map_err(|_| incompatible_error("stored reset-card credential writer is invalid"))?;
	let provider_kind = match required_str(operation, "provider_kind")? {
		"chatgpt" => AccountProvider::Chatgpt,
		_ => return incompatible("stored reset-card provider kind is invalid"),
	};
	let provider =
		ProviderIdentity::new(provider_kind, required_str(operation, "provider_account_id")?)
			.map_err(|_| incompatible_error("stored reset-card provider identity is invalid"))?;
	let credential = CredentialBinding {
		schema_version: schema,
		version,
		fingerprint,
		provider,
		writer_operation_id,
	};
	ProcessGenerationAccountBinding::new(
		account_revision,
		credential,
		required_str(operation, "refresh_callback_profile_sha256")?,
	)
	.map_err(|_| incompatible_error("stored reset-card process binding is invalid"))
}

fn provider_text(provider: AccountProvider) -> &'static str {
	match provider {
		AccountProvider::Chatgpt => "chatgpt",
	}
}

fn preparation_response(
	account_id: &AccountId,
	account_revision: i64,
	descriptor: ResetCardDescriptor,
) -> Value {
	json!({
		"kind": "reset_card_operation",
		"account_id": account_id.as_str(),
		"account_revision": account_revision,
		"granted_at_unix_seconds": descriptor.granted_at().unix_seconds(),
		"expires_at_unix_seconds": descriptor.expires_at().unix_seconds(),
		"state": "prepared",
	})
}

fn rejection_response(account_id: &AccountId, rejection: ResetCardPreparationRejection) -> Value {
	match rejection {
		ResetCardPreparationRejection::NotEnrolled => json!({
			"kind": "reset_card_operation",
			"state": "rejected_before_effect",
			"account_id": account_id.as_str(),
			"error_code": "account_not_enrolled",
		}),
		ResetCardPreparationRejection::RevisionChanged { expected, actual } => json!({
			"kind": "reset_card_operation",
			"state": "rejected_before_effect",
			"account_id": account_id.as_str(),
			"error_code": "account_revision_changed",
			"expected_revision": expected,
			"actual_revision": actual,
		}),
		ResetCardPreparationRejection::StateRejected => json!({
			"kind": "reset_card_operation",
			"state": "rejected_before_effect",
			"account_id": account_id.as_str(),
			"error_code": "account_state_rejected",
		}),
	}
}

fn preparation_from_response(response: Value) -> Result<ResetCardPreparation, StoreError> {
	let object = response
		.as_object()
		.ok_or_else(|| incompatible_error("stored reset-card command response is malformed"))?;
	if object.get("kind").and_then(Value::as_str) != Some("reset_card_operation") {
		return Err(StoreError::IdempotencyConflict);
	}
	let account_id = AccountId::new(required_str(object, "account_id")?)
		.map_err(|_| incompatible_error("stored reset-card account identity is invalid"))?;
	match object.get("state").and_then(Value::as_str) {
		Some("prepared") => {
			let account_revision = required_i64(object, "account_revision")?;
			let descriptor = stored_descriptor(object)?;

			Ok(ResetCardPreparation { account_id, account_revision, descriptor })
		},
		Some("rejected_before_effect") => {
			let rejection = match required_str(object, "error_code")? {
				"account_not_enrolled" => ResetCardPreparationRejection::NotEnrolled,
				"account_revision_changed" => ResetCardPreparationRejection::RevisionChanged {
					expected: required_i64(object, "expected_revision")?,
					actual: required_i64(object, "actual_revision")?,
				},
				"account_state_rejected" => ResetCardPreparationRejection::StateRejected,
				_ => return incompatible("stored reset-card preparation rejection is unknown"),
			};

			Err(rejection.store_error(&account_id))
		},
		_ => Err(StoreError::IdempotencyConflict),
	}
}

fn stored_descriptor(
	object: &serde_json::Map<String, Value>,
) -> Result<ResetCardDescriptor, StoreError> {
	let granted =
		ResetCardTimestamp::from_unix_seconds(required_i64(object, "granted_at_unix_seconds")?)
			.map_err(|_| incompatible_error("stored reset-card grant timestamp is invalid"))?;
	let expires =
		ResetCardTimestamp::from_unix_seconds(required_i64(object, "expires_at_unix_seconds")?)
			.map_err(|_| incompatible_error("stored reset-card expiry timestamp is invalid"))?;

	ResetCardDescriptor::new(granted, expires)
		.map_err(|_| incompatible_error("stored reset-card descriptor is invalid"))
}

fn descriptor_digest(descriptor: ResetCardDescriptor) -> String {
	use sha2::{Digest as _, Sha256};

	let source = format!(
		"{}:{}",
		descriptor.granted_at().unix_seconds(),
		descriptor.expires_at().unix_seconds()
	);

	Sha256::digest(source.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn reset_card_command_descriptor(
	account_id: &AccountId,
	expected_revision: i64,
	descriptor: ResetCardDescriptor,
) -> CommandDescriptor {
	CommandDescriptor {
		protocol_version: STORE_COMMAND_PROTOCOL,
		operation: "consume_reset_card",
		project_scope: "global",
		scope_id: "reset_cards".into(),
		entity_id: account_id.as_str().into(),
		expected_revision: Some(expected_revision),
		payload_hash: Some(descriptor_digest(descriptor)),
		payload_length: None,
	}
}

async fn ensure_operation_exists(
	client: &deadpool_postgres::Client,
	idempotency_key: &str,
) -> Result<(), StoreError> {
	let aggregate_id = operation_aggregate_id(idempotency_key);
	let exists: bool = client
		.query_one(
			"SELECT EXISTS(SELECT 1 FROM decodex.outbox \
			 WHERE aggregate_kind='reset_card_operation' AND aggregate_id=$1)",
			&[&aggregate_id],
		)
		.await?
		.get(0);

	if exists {
		Ok(())
	} else {
		incompatible("completed reset-card command lost its durable operation")
	}
}

fn account_state(value: &str) -> Result<AccountState, StoreError> {
	match value {
		"unavailable" => Ok(AccountState::Unavailable),
		"unknown" => Ok(AccountState::Unknown),
		"available" => Ok(AccountState::Available),
		"depleted" => Ok(AccountState::Depleted),
		"auth_failed" => Ok(AccountState::AuthFailed),
		"plugin_unready" => Ok(AccountState::PluginUnready),
		_ => incompatible("stored account state is invalid"),
	}
}

fn valid_exact_credit_id(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= MAX_EXACT_CREDIT_ID_BYTES
		&& value.trim() == value
		&& !value.chars().any(char::is_control)
}

fn operation_aggregate_id(idempotency_key: &str) -> String {
	use sha2::{Digest as _, Sha256};

	let mut digest = Sha256::new();
	digest.update(b"decodex/reset-card-operation-id/1\0");
	digest.update(idempotency_key.as_bytes());

	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn encode_private_text(value: &str) -> String {
	use std::fmt::Write as _;

	let mut encoded = String::with_capacity(value.len() * 2);
	for byte in value.as_bytes() {
		write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
	}

	encoded
}

fn decode_exact_credit_id(encoded: &str) -> Result<String, StoreError> {
	let value = decode_private_text(
		encoded,
		MAX_EXACT_CREDIT_ID_BYTES,
		"stored reset-card exact credit binding is invalid",
	)?;

	if valid_exact_credit_id(&value) {
		Ok(value)
	} else {
		incompatible("stored reset-card exact credit binding is invalid")
	}
}

fn decode_private_text(
	encoded: &str,
	maximum_bytes: usize,
	error: &'static str,
) -> Result<String, StoreError> {
	if encoded.len() > maximum_bytes * 2 || !encoded.len().is_multiple_of(2) {
		return incompatible(error);
	}
	let mut bytes = Vec::with_capacity(encoded.len() / 2);
	for pair in encoded.as_bytes().chunks_exact(2) {
		let high = decode_hex_nibble(pair[0]).ok_or_else(|| incompatible_error(error))?;
		let low = decode_hex_nibble(pair[1]).ok_or_else(|| incompatible_error(error))?;
		bytes.push((high << 4) | low);
	}
	String::from_utf8(bytes).map_err(|_| incompatible_error(error))
}

const fn decode_hex_nibble(byte: u8) -> Option<u8> {
	match byte {
		b'0'..=b'9' => Some(byte - b'0'),
		b'a'..=b'f' => Some(byte - b'a' + 10),
		_ => None,
	}
}

fn required_str<'a>(
	object: &'a serde_json::Map<String, Value>,
	key: &str,
) -> Result<&'a str, StoreError> {
	object
		.get(key)
		.and_then(Value::as_str)
		.ok_or_else(|| incompatible_error("stored reset-card text field is malformed"))
}

fn required_i64(object: &serde_json::Map<String, Value>, key: &str) -> Result<i64, StoreError> {
	object
		.get(key)
		.and_then(Value::as_i64)
		.ok_or_else(|| incompatible_error("stored reset-card integer field is malformed"))
}

fn incompatible<T>(message: &'static str) -> Result<T, StoreError> {
	Err(incompatible_error(message))
}

fn incompatible_error(message: &'static str) -> StoreError {
	StoreError::Incompatible(message.into())
}

#[cfg(test)]
mod tests {
	use super::{
		RENEW_RESET_CARD_CLAIM_SQL, ResetCardClaim, ResetCardFailureCode, UnknownCommitReadback,
		classify_unknown_commit_readback, decode_exact_credit_id, descriptor_digest,
		encode_private_text, operation_aggregate_id, operation_public_payload,
	};
	use crate::{AccountId, StoreError};
	use decodex_core::{
		AccountOperationId, AccountProvider, CredentialBinding, CredentialFingerprint,
		CredentialStoreSchemaVersion, CredentialVersion, ProcessGenerationAccountBinding,
		ProviderIdentity, ResetCardConsumeOutcome, ResetCardDescriptor, ResetCardTimestamp,
	};

	fn descriptor(granted: i64, expires: i64) -> ResetCardDescriptor {
		ResetCardDescriptor::new(
			ResetCardTimestamp::from_unix_seconds(granted).unwrap(),
			ResetCardTimestamp::from_unix_seconds(expires).unwrap(),
		)
		.unwrap()
	}

	fn process_binding(revision: i64) -> ProcessGenerationAccountBinding {
		ProcessGenerationAccountBinding::new(
			revision,
			CredentialBinding {
				schema_version: CredentialStoreSchemaVersion::V1,
				version: CredentialVersion::new(1).unwrap(),
				fingerprint: CredentialFingerprint::new(
					"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
				)
				.unwrap(),
				provider: ProviderIdentity::new(AccountProvider::Chatgpt, "provider-account")
					.unwrap(),
				writer_operation_id: AccountOperationId::new(
					"71000000-0000-4000-8000-000000000010",
				)
				.unwrap(),
			},
			"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
		)
		.unwrap()
	}

	#[test]
	fn public_descriptor_digest_is_stable_and_separates_exact_times() {
		assert_eq!(descriptor_digest(descriptor(100, 200)).len(), 64);
		assert_ne!(
			descriptor_digest(descriptor(100, 200)),
			descriptor_digest(descriptor(100, 201))
		);
	}

	#[test]
	fn public_operation_identity_is_stable_and_does_not_embed_the_provider_key() {
		let provider_key = "private-provider-retry-marker";
		let aggregate_id = operation_aggregate_id(provider_key);

		assert_eq!(aggregate_id.len(), 64);
		assert_eq!(aggregate_id, operation_aggregate_id(provider_key));
		assert_ne!(aggregate_id, operation_aggregate_id("different-provider-key"));
		assert!(!aggregate_id.contains(provider_key));
	}

	#[test]
	fn persisted_failure_codes_are_closed_and_round_trip() {
		for code in [
			ResetCardFailureCode::AccountChanged,
			ResetCardFailureCode::VaultUnavailable,
			ResetCardFailureCode::SchemaUnsupported,
			ResetCardFailureCode::InventoryIncomplete,
			ResetCardFailureCode::InventoryChanged,
			ResetCardFailureCode::ProviderUnavailable,
			ResetCardFailureCode::ResourceExhausted,
		] {
			assert_eq!(ResetCardFailureCode::parse(code.as_str()).unwrap(), code);
		}
	}

	#[test]
	fn public_operation_payload_excludes_private_effect_material() {
		let account_id = AccountId::new("10000000-0000-0000-0000-000000000001").unwrap();
		let binding = process_binding(7);
		let payload = operation_public_payload(&account_id, 7, &binding, descriptor(100, 200));
		let rendered = payload.to_string();

		assert_eq!(
			payload.get("schema").and_then(serde_json::Value::as_str),
			Some(super::PROTOCOL)
		);
		assert!(!rendered.contains("exact_credit"));
		assert!(!rendered.contains("provider_idempotency"));
		assert!(!rendered.contains("reset_card_effect"));
		assert!(!rendered.contains(binding.credential.writer_operation_id.as_str()));
	}

	#[test]
	fn private_effect_identity_uses_strict_reversible_credential_negative_encoding() {
		let exact_id = "sk-live-provider-id";
		let encoded = encode_private_text(exact_id);

		assert_ne!(encoded, exact_id);
		assert!(encoded.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
		assert!(crate::ensure_credential_negative_text(&encoded).is_ok());
		assert_eq!(decode_exact_credit_id(&encoded).unwrap(), exact_id);
		assert!(decode_exact_credit_id("ABC0").is_err());
		assert!(decode_exact_credit_id("0").is_err());
	}

	#[test]
	fn reset_card_claim_debug_redacts_all_private_owner_and_provider_values() {
		let claim = ResetCardClaim {
			id: 17,
			claim_token: "private-claim-token".into(),
			account_id: AccountId::new("10000000-0000-0000-0000-000000000001").unwrap(),
			account_revision: 3,
			process_binding: process_binding(3),
			descriptor: descriptor(100, 200),
			exact_credit_id: Some("private-provider-credit".into()),
			provider_idempotency_key: "private-provider-retry".into(),
			requires_reconciliation: true,
			recorded_outcome: Some(ResetCardConsumeOutcome::Reset),
		};
		let rendered = format!("{claim:?}");

		for private in [
			claim.claim_token(),
			claim.exact_credit_id().unwrap(),
			claim.provider_idempotency_key(),
		] {
			assert!(!rendered.contains(private));
		}
		assert!(rendered.contains("exact_credit_id_bound: true"));
		assert!(rendered.contains("receipt_recorded: true"));
	}

	#[test]
	fn typed_lease_renewal_is_fenced_to_live_reset_card_ownership() {
		for required in [
			"aggregate_kind='reset_card_operation'",
			"state='in_flight'",
			"lease_holder=$2::text::uuid",
			"claim_token=$3::text::uuid",
			"lease_expires_at > write_time.value",
		] {
			assert!(RENEW_RESET_CARD_CLAIM_SQL.contains(required), "{required}");
		}
	}

	#[test]
	fn unknown_preparation_commit_requires_conclusive_same_key_readback() {
		assert!(matches!(
			classify_unknown_commit_readback::<u8>(Ok(Some(7))),
			UnknownCommitReadback::Committed(7),
		));
		assert!(matches!(
			classify_unknown_commit_readback::<u8>(Ok(None)),
			UnknownCommitReadback::Unresolved,
		));
		assert!(matches!(
			classify_unknown_commit_readback::<u8>(Err(StoreError::SocketUnavailable)),
			UnknownCommitReadback::Unresolved,
		));
	}
}
