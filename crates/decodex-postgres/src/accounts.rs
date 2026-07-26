use std::time::Duration;

use deadpool_postgres::{Client, Transaction};
use serde_json::{self, Value};
use tokio::time::{self, Instant};
use tokio_postgres::Row;

use crate::{
	AccountId, AccountMetadata, AccountMutation, AccountState, ActivityRecord, CommandIdentity,
	PostgresStore, StoreError,
};
use decodex_core::RESET_CARD_PROVIDER_BINDING_METADATA_FIELD;

/// Immutable identity bound to one durable command receipt before side effects begin.
pub(crate) struct CommandDescriptor {
	pub protocol_version: &'static str,
	pub operation: &'static str,
	pub project_scope: &'static str,
	pub scope_id: String,
	pub entity_id: String,
	pub expected_revision: Option<i64>,
	pub payload_hash: Option<String>,
	pub payload_length: Option<i64>,
}

/// Fenced ownership of one pending receipt.
pub(crate) struct CommandReservation {
	key: String,
	request_hash: String,
	claim_token: String,
}

/// A reservation either owns the pending saga or replays its exact completed response bytes.
pub(crate) enum CommandClaim {
	Owned(CommandReservation),
	Completed(Value),
}

/// Read-only state of one exact command receipt.
pub(crate) enum CommandReplay {
	Absent,
	Pending { claim_expired: bool },
	Completed(Value),
}

impl PostgresStore {
	/// Observe whether one explicitly named account admits manual reset-card use at an exact
	/// revision.
	///
	/// This check deliberately admits both `available` and `depleted`. It never selects an
	/// account, and it releases its database checkout before caller-controlled process work.
	pub async fn account_admits_reset_card_at_revision(
		&self,
		account_id: &AccountId,
		expected_revision: i64,
	) -> Result<bool, StoreError> {
		if expected_revision < 1 {
			return Err(StoreError::InvalidInput("expected revision must be positive"));
		}

		let client = self.pool().get().await?;
		let row = client
			.query_one(
				"SELECT EXISTS (SELECT 1 FROM decodex.accounts \
				 WHERE account_id=$1::text::uuid AND revision=$2 \
				 AND state IN ('available'::decodex.account_state, \
				               'depleted'::decodex.account_state))",
				&[&account_id.as_str(), &expected_revision],
			)
			.await?;
		let admitted = row.get(0);

		drop(row);
		drop(client);

		Ok(admitted)
	}

	/// Observe whether one explicitly named account is available at an exact revision.
	///
	/// This operation never selects an account, returns no retainable authority, and invokes no
	/// caller work while a database client is checked out. Callers must re-observe after any
	/// potentially blocking mechanical work before constructing a product result.
	pub async fn account_is_ready_at_revision(
		&self,
		account_id: &AccountId,
		expected_revision: i64,
	) -> Result<bool, StoreError> {
		if expected_revision < 1 {
			return Err(StoreError::InvalidInput("expected revision must be positive"));
		}

		let client = self.pool().get().await?;
		let row = client
			.query_one(
				"SELECT EXISTS (SELECT 1 FROM decodex.accounts \
				 WHERE account_id=$1::text::uuid AND revision=$2 \
				 AND state='available'::decodex.account_state)",
				&[&account_id.as_str(), &expected_revision],
			)
			.await?;
		let ready = row.get(0);

		// Make the checkout boundary explicit: no caller-controlled vault or process work may
		// begin until both the result row and its pooled client have been released.
		drop(row);
		drop(client);

		Ok(ready)
	}

	/// Apply one inert account metadata mutation. The account revision, append-only activity,
	/// outbox event, and command response commit in one PostgreSQL transaction.
	pub async fn mutate_account(
		&self,
		command: &CommandIdentity,
		mutation: &AccountMutation,
	) -> Result<AccountMetadata, StoreError> {
		validate_account(mutation)?;

		let mut client = self.pool().get().await?;
		let descriptor = CommandDescriptor {
			protocol_version: "decodex/store-command/1",
			operation: "mutate_account",
			project_scope: "global",
			scope_id: "accounts".into(),
			entity_id: mutation.account_id.as_str().into(),
			expected_revision: mutation.expected_revision,
			payload_hash: None,
			payload_length: None,
		};
		let reservation = match reserve_command(&mut client, command, &descriptor).await? {
			CommandClaim::Completed(response) => return account_from_response(response),
			CommandClaim::Owned(reservation) => reservation,
		};
		let transaction = client.transaction().await?;
		let revision = match mutation.expected_revision {
			None => create_account(&transaction, mutation).await?,
			Some(expected) => update_account(&transaction, mutation, expected).await?,
		};
		let event_kind = if revision == 1 { "account_created" } else { "account_updated" };
		let payload = serde_json::json!({
			"account_id": mutation.account_id.as_str(),
			"state": account_state_sql(mutation.state),
			"revision": revision,
		});

		append_activity_and_outbox(
			&transaction,
			"account",
			mutation.account_id.as_str(),
			revision,
			event_kind,
			&command.key,
			&payload,
		)
		.await?;

		let response = serde_json::json!({
			"kind": "account",
			"account_id": mutation.account_id.as_str(),
			"display_label": mutation.display_label,
			"state": account_state_sql(mutation.state),
			"metadata": mutation.metadata,
			"revision": revision,
		});

		finish_command(&transaction, &reservation, &response).await?;

		transaction.commit().await?;

		account_from_response(response)
	}

	/// Read account metadata without providing an eligibility or selection query.
	pub async fn account(
		&self,
		account_id: &AccountId,
	) -> Result<Option<AccountMetadata>, StoreError> {
		let row = self
			.pool()
			.get()
			.await?
			.query_opt(
				"SELECT account_id::text, display_label, state::text, metadata, revision \
				 FROM decodex.accounts WHERE account_id = $1::text::uuid",
				&[&account_id.as_str()],
			)
			.await?;

		row.map(account_from_row).transpose()
	}

	/// Read append-only activity in sequence order for validation and future application use.
	pub async fn activity_after(
		&self,
		after_sequence: i64,
		limit: u32,
	) -> Result<Vec<ActivityRecord>, StoreError> {
		let limit = i64::from(limit.min(1_000));
		let rows = self
			.pool()
			.get()
			.await?
			.query(
				"SELECT sequence, aggregate_kind, aggregate_id, revision, event_kind, payload \
				 FROM decodex.activity WHERE sequence > $1 ORDER BY sequence LIMIT $2",
				&[&after_sequence, &limit],
			)
			.await?;

		Ok(rows
			.into_iter()
			.map(|row| ActivityRecord {
				sequence: row.get(0),
				aggregate_kind: row.get(1),
				aggregate_id: row.get(2),
				revision: row.get(3),
				event_kind: row.get(4),
				payload: row.get(5),
			})
			.collect())
	}
}

pub(crate) async fn reserve_command(
	client: &mut Client,
	command: &CommandIdentity,
	descriptor: &CommandDescriptor,
) -> Result<CommandClaim, StoreError> {
	let wait_deadline = Instant::now() + Duration::from_secs(30);

	loop {
		let transaction = client.transaction().await?;
		let inserted = transaction
			.query_opt(
				"INSERT INTO decodex.command_receipts \
			 (idempotency_key, request_hash, protocol_version, operation, project_scope, scope_id, \
			  entity_id, expected_revision, payload_hash, payload_length, receipt_state, \
			  claim_token, claim_expires_at) \
			 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'pending',gen_random_uuid(), \
			  clock_timestamp()+interval '5 minutes') \
			 ON CONFLICT DO NOTHING RETURNING claim_token::text",
				&[
					&command.key,
					&command.request_hash,
					&descriptor.protocol_version,
					&descriptor.operation,
					&descriptor.project_scope,
					&descriptor.scope_id,
					&descriptor.entity_id,
					&descriptor.expected_revision,
					&descriptor.payload_hash,
					&descriptor.payload_length,
				],
			)
			.await?;

		if let Some(row) = inserted {
			let claim_token = row.get(0);

			transaction.commit().await?;

			return Ok(CommandClaim::Owned(CommandReservation {
				key: command.key.clone(),
				request_hash: command.request_hash.clone(),
				claim_token,
			}));
		}

		let row = transaction
			.query_one(
				"SELECT request_hash, protocol_version, operation, project_scope, scope_id, entity_id, \
			 expected_revision, payload_hash, payload_length, receipt_state::text, response_bytes, \
			 claim_expires_at <= clock_timestamp() \
			 FROM decodex.command_receipts \
			 WHERE idempotency_key = $1 FOR UPDATE",
				&[&command.key],
			)
			.await?;

		if !receipt_descriptor_matches(&row, command, descriptor) {
			return Err(StoreError::IdempotencyConflict);
		}
		if row.get::<_, &str>(9) == "completed" {
			let bytes = row.get::<_, Option<Vec<u8>>>(10).ok_or_else(|| {
				StoreError::Incompatible("completed command receipt lost response bytes".into())
			})?;
			let response: Value = serde_json::from_slice(&bytes).map_err(|_| {
				StoreError::Incompatible("completed command response bytes are invalid".into())
			})?;
			let stored_response: Value = transaction
				.query_one(
					"SELECT response FROM decodex.command_receipts WHERE idempotency_key=$1",
					&[&command.key],
				)
				.await?
				.get(0);

			if response != stored_response {
				return Err(StoreError::Incompatible(
					"completed command response bytes differ from committed response".into(),
				));
			}

			transaction.commit().await?;

			return Ok(CommandClaim::Completed(response));
		}
		if !row.get::<_, bool>(11) {
			transaction.rollback().await?;

			if Instant::now() >= wait_deadline {
				return Err(StoreError::OwnershipLost("command receipt claim is active"));
			}

			time::sleep(Duration::from_millis(10)).await;

			continue;
		}

		let claim_token: String = transaction
			.query_one(
				"UPDATE decodex.command_receipts SET claim_token=gen_random_uuid(), \
			 claim_expires_at=clock_timestamp()+interval '5 minutes' \
			 WHERE idempotency_key=$1 AND receipt_state='pending' \
			 RETURNING claim_token::text",
				&[&command.key],
			)
			.await?
			.get(0);

		transaction.commit().await?;

		return Ok(CommandClaim::Owned(CommandReservation {
			key: command.key.clone(),
			request_hash: command.request_hash.clone(),
			claim_token,
		}));
	}
}

pub(crate) async fn replay_completed_command(
	client: &Client,
	command: &CommandIdentity,
	descriptor: &CommandDescriptor,
) -> Result<Option<Value>, StoreError> {
	match inspect_command_receipt(client, command, descriptor).await? {
		CommandReplay::Completed(response) => Ok(Some(response)),
		CommandReplay::Absent | CommandReplay::Pending { .. } => Ok(None),
	}
}

pub(crate) async fn inspect_command_receipt(
	client: &Client,
	command: &CommandIdentity,
	descriptor: &CommandDescriptor,
) -> Result<CommandReplay, StoreError> {
	let Some(row) = client
		.query_opt(
			"SELECT request_hash, protocol_version, operation, project_scope, scope_id, entity_id, \
			 expected_revision, payload_hash, payload_length, receipt_state::text, response_bytes, \
			 response, claim_expires_at <= clock_timestamp() \
			 FROM decodex.command_receipts WHERE idempotency_key=$1",
			&[&command.key],
		)
		.await?
	else {
		return Ok(CommandReplay::Absent);
	};

	if !receipt_descriptor_matches(&row, command, descriptor) {
		return Err(StoreError::IdempotencyConflict);
	}
	if row.get::<_, &str>(9) != "completed" {
		return Ok(CommandReplay::Pending { claim_expired: row.get(12) });
	}

	let bytes = row.get::<_, Option<Vec<u8>>>(10).ok_or_else(|| {
		StoreError::Incompatible("completed command receipt lost response bytes".into())
	})?;
	let response: Value = serde_json::from_slice(&bytes).map_err(|_| {
		StoreError::Incompatible("completed command response bytes are invalid".into())
	})?;
	let stored_response: Value = row.get(11);

	if response != stored_response {
		return Err(StoreError::Incompatible(
			"completed command response bytes differ from committed response".into(),
		));
	}

	Ok(CommandReplay::Completed(response))
}

pub(crate) async fn finish_command(
	transaction: &Transaction<'_>,
	reservation: &CommandReservation,
	response: &Value,
) -> Result<(), StoreError> {
	let response_bytes = serde_json::to_vec(response)
		.map_err(|_| StoreError::InvalidInput("command response cannot be serialized"))?;
	let updated = transaction
		.execute(
			"UPDATE decodex.command_receipts SET response=$2, response_bytes=$3, \
			 receipt_state='completed', completed_at=clock_timestamp(), \
			 completion_claim_token=$5::text::uuid, claim_token=NULL, \
			 claim_expires_at=NULL \
			 WHERE idempotency_key=$1 AND request_hash=$4 AND receipt_state='pending' \
			 AND claim_token=$5::text::uuid AND claim_expires_at > clock_timestamp()",
			&[
				&reservation.key,
				response,
				&response_bytes,
				&reservation.request_hash,
				&reservation.claim_token,
			],
		)
		.await?;

	if updated == 1 { Ok(()) } else { Err(StoreError::OwnershipLost("command receipt claim")) }
}

pub(crate) async fn append_activity_and_outbox(
	transaction: &Transaction<'_>,
	aggregate_kind: &str,
	aggregate_id: &str,
	revision: i64,
	event_kind: &str,
	correlation_key: &str,
	payload: &Value,
) -> Result<(), StoreError> {
	let sequence: i64 = transaction
		.query_one(
			"INSERT INTO decodex.activity \
			 (aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload) \
			 VALUES ($1, $2, $3, $4, $5, $6) RETURNING sequence",
			&[&aggregate_kind, &aggregate_id, &revision, &event_kind, &correlation_key, payload],
		)
		.await?
		.get(0);
	let outbox_payload = serde_json::json!({
		"activity_sequence": sequence,
		"event_kind": event_kind,
		"aggregate_kind": aggregate_kind,
		"aggregate_id": aggregate_id,
		"revision": revision,
		"payload": payload,
	});
	let effect_key = format!("activity/{sequence}");

	transaction
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload) \
			 VALUES ($1, $2, $3, $4, $5)",
			&[&effect_key, &aggregate_kind, &aggregate_id, &revision, &outbox_payload],
		)
		.await?;

	Ok(())
}

fn receipt_descriptor_matches(
	row: &Row,
	command: &CommandIdentity,
	descriptor: &CommandDescriptor,
) -> bool {
	row.get::<_, String>(0) == command.request_hash
		&& row.get::<_, String>(1) == descriptor.protocol_version
		&& row.get::<_, String>(2) == descriptor.operation
		&& row.get::<_, String>(3) == descriptor.project_scope
		&& row.get::<_, String>(4) == descriptor.scope_id
		&& row.get::<_, String>(5) == descriptor.entity_id
		&& row.get::<_, Option<i64>>(6) == descriptor.expected_revision
		&& row.get::<_, Option<String>>(7) == descriptor.payload_hash
		&& row.get::<_, Option<i64>>(8) == descriptor.payload_length
}

const fn account_state_sql(state: AccountState) -> &'static str {
	match state {
		AccountState::Unavailable => "unavailable",
		AccountState::Unknown => "unknown",
		AccountState::Available => "available",
		AccountState::Depleted => "depleted",
		AccountState::AuthFailed => "auth_failed",
		AccountState::PluginUnready => "plugin_unready",
		AccountState::Disabled => "disabled",
	}
}

fn account_state_from_sql(value: &str) -> Result<AccountState, StoreError> {
	match value {
		"unavailable" => Ok(AccountState::Unavailable),
		"unknown" => Ok(AccountState::Unknown),
		"available" => Ok(AccountState::Available),
		"depleted" => Ok(AccountState::Depleted),
		"auth_failed" => Ok(AccountState::AuthFailed),
		"plugin_unready" => Ok(AccountState::PluginUnready),
		"disabled" => Ok(AccountState::Disabled),
		_ => Err(StoreError::Incompatible(format!("unknown account state {value}"))),
	}
}

fn validate_account(mutation: &AccountMutation) -> Result<(), StoreError> {
	if mutation.display_label.is_empty() || mutation.display_label.len() > 128 {
		return Err(StoreError::InvalidInput("account display label must contain 1..=128 bytes"));
	}
	if mutation.expected_revision.is_some_and(|revision| revision < 1) {
		return Err(StoreError::InvalidInput("expected revision must be positive"));
	}

	crate::ensure_credential_negative_text(&mutation.display_label)?;
	crate::ensure_credential_negative_json(&mutation.metadata)?;

	Ok(())
}

fn account_from_row(row: Row) -> Result<AccountMetadata, StoreError> {
	Ok(AccountMetadata {
		account_id: stored_account_id(row.get::<_, String>(0))?,
		display_label: row.get(1),
		state: account_state_from_sql(row.get::<_, &str>(2))?,
		metadata: row.get(3),
		revision: row.get(4),
	})
}

fn account_from_response(response: Value) -> Result<AccountMetadata, StoreError> {
	if response.get("kind").and_then(Value::as_str) != Some("account") {
		return Err(StoreError::IdempotencyConflict);
	}

	Ok(AccountMetadata {
		account_id: stored_account_id(required_str(&response, "account_id")?)?,
		display_label: required_str(&response, "display_label")?.to_owned(),
		state: account_state_from_sql(required_str(&response, "state")?)?,
		metadata: response
			.get("metadata")
			.cloned()
			.ok_or(StoreError::Incompatible("account response missing metadata".into()))?,
		revision: required_i64(&response, "revision")?,
	})
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	value
		.get(key)
		.and_then(Value::as_str)
		.ok_or_else(|| StoreError::Incompatible(format!("command response missing {key}")))
}

fn required_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_i64)
		.ok_or_else(|| StoreError::Incompatible(format!("command response missing {key}")))
}

fn stored_account_id(value: impl Into<String>) -> Result<AccountId, StoreError> {
	AccountId::new(value)
		.map_err(|_| StoreError::Incompatible("stored account identity is invalid".into()))
}

async fn create_account(
	transaction: &Transaction<'_>,
	mutation: &AccountMutation,
) -> Result<i64, StoreError> {
	let result = transaction
		.query_opt(
			"INSERT INTO decodex.accounts (account_id, display_label, state, metadata) \
			 VALUES ($1::text::uuid, $2, $3::text::decodex.account_state, $4) \
			 ON CONFLICT DO NOTHING RETURNING revision",
			&[
				&mutation.account_id.as_str(),
				&mutation.display_label,
				&account_state_sql(mutation.state),
				&mutation.metadata,
			],
		)
		.await?;

	if let Some(row) = result {
		return Ok(row.get(0));
	}

	let actual = account_revision(transaction, &mutation.account_id).await?;

	Err(StoreError::RevisionConflict {
		entity: format!("account/{}", mutation.account_id),
		expected: None,
		actual,
	})
}

async fn update_account(
	transaction: &Transaction<'_>,
	mutation: &AccountMutation,
	expected: i64,
) -> Result<i64, StoreError> {
	let row = transaction
		.query_opt(
			"UPDATE decodex.accounts SET display_label = $2, state = $3::text::decodex.account_state, \
			 metadata = $4, revision = revision + 1, observed_at = clock_timestamp(), \
			 updated_at = clock_timestamp() WHERE account_id = $1::text::uuid AND revision = $5 \
			 AND (NOT metadata ? ($6::text) \
			   OR (metadata -> ($6::text)) = ($4::jsonb -> ($6::text))) \
			 RETURNING revision",
			&[
				&mutation.account_id.as_str(),
				&mutation.display_label,
				&account_state_sql(mutation.state),
				&mutation.metadata,
				&expected,
				&RESET_CARD_PROVIDER_BINDING_METADATA_FIELD,
			],
		)
		.await?;

	if let Some(row) = row {
		return Ok(row.get(0));
	}

	let actual = account_revision(transaction, &mutation.account_id).await?;

	Err(StoreError::RevisionConflict {
		entity: format!("account/{}", mutation.account_id),
		expected: Some(expected),
		actual,
	})
}

async fn account_revision(
	transaction: &Transaction<'_>,
	account_id: &AccountId,
) -> Result<Option<i64>, StoreError> {
	Ok(transaction
		.query_opt(
			"SELECT revision FROM decodex.accounts WHERE account_id = $1::text::uuid",
			&[&account_id.as_str()],
		)
		.await?
		.map(|row| row.get(0)))
}

#[cfg(test)]
mod tests {
	use crate::accounts::{self};
	use decodex_core::AccountState;

	#[test]
	fn every_typed_account_observation_has_one_storage_value() {
		for (state, stored) in [
			(AccountState::Unavailable, "unavailable"),
			(AccountState::Unknown, "unknown"),
			(AccountState::Available, "available"),
			(AccountState::Depleted, "depleted"),
			(AccountState::AuthFailed, "auth_failed"),
			(AccountState::PluginUnready, "plugin_unready"),
			(AccountState::Disabled, "disabled"),
		] {
			assert_eq!(accounts::account_state_sql(state), stored);
			assert_eq!(accounts::account_state_from_sql(stored).unwrap(), state);
		}
	}
}
