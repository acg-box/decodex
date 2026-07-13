use deadpool_postgres::Transaction;
use serde_json::{self, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio_postgres::Row;

use crate::{
	AccountId, AccountMetadata, AccountMutation, AccountState, ActivityRecord, CommandIdentity,
	PostgresStore, QuotaWindow, QuotaWindowMutation, StoreError,
};

impl PostgresStore {
	/// Apply one inert account metadata mutation. The account revision, append-only activity,
	/// outbox event, and command response commit in one PostgreSQL transaction.
	pub async fn mutate_account(
		&self,
		command: &CommandIdentity,
		mutation: &AccountMutation,
	) -> Result<AccountMetadata, StoreError> {
		validate_account(mutation)?;

		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;

		if let Some(response) = begin_command(&transaction, command).await? {
			let account = account_from_response(response)?;

			transaction.commit().await?;

			return Ok(account);
		}

		let revision = match mutation.expected_revision {
			None => create_account(&transaction, mutation).await?,
			Some(expected) => update_account(&transaction, mutation, expected).await?,
		};
		let event_kind = if revision == 1 { "account_created" } else { "account_updated" };
		let payload = serde_json::json!({
			"account_id": mutation.account_id.as_str(),
			"state": mutation.state.as_sql(),
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
			"state": mutation.state.as_sql(),
			"metadata": mutation.metadata,
			"revision": revision,
		});

		finish_command(&transaction, command, &response).await?;

		transaction.commit().await?;

		account_from_response(response)
	}

	/// Apply one inert duration-typed quota-window mutation. No eligibility, assignment,
	/// fallback, or wake decision is exposed by this storage boundary.
	pub async fn mutate_quota_window(
		&self,
		command: &CommandIdentity,
		mutation: &QuotaWindowMutation,
	) -> Result<QuotaWindow, StoreError> {
		validate_window(mutation)?;

		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;

		if let Some(response) = begin_command(&transaction, command).await? {
			let window = window_from_response(response)?;

			transaction.commit().await?;

			return Ok(window);
		}

		let revision = match mutation.expected_revision {
			None => create_window(&transaction, mutation).await?,
			Some(expected) => update_window(&transaction, mutation, expected).await?,
		};
		let aggregate_id = format!(
			"{}:{}/{}",
			mutation.account_id, mutation.window_class, mutation.duration_seconds
		);
		let event_kind =
			if revision == 1 { "quota_window_created" } else { "quota_window_updated" };
		let payload = serde_json::json!({
			"account_id": mutation.account_id.as_str(),
			"window_class": mutation.window_class,
			"duration_seconds": mutation.duration_seconds,
			"revision": revision,
		});

		append_activity_and_outbox(
			&transaction,
			"quota_window",
			&aggregate_id,
			revision,
			event_kind,
			&command.key,
			&payload,
		)
		.await?;

		let response = serde_json::json!({
			"kind": "quota_window",
			"account_id": mutation.account_id.as_str(),
			"window_class": mutation.window_class,
			"duration_seconds": mutation.duration_seconds,
			"remaining_amount": mutation.remaining_amount,
			"resets_at": mutation.resets_at,
			"observed_at": mutation.observed_at,
			"confidence": mutation.confidence,
			"metadata": mutation.metadata,
			"revision": revision,
		});

		finish_command(&transaction, command, &response).await?;

		transaction.commit().await?;

		window_from_response(response)
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

fn validate_window(mutation: &QuotaWindowMutation) -> Result<(), StoreError> {
	if mutation.duration_seconds <= 0 {
		return Err(StoreError::InvalidInput("quota window duration must be positive"));
	}
	if !(0.0..=1.0).contains(&mutation.confidence) || !mutation.confidence.is_finite() {
		return Err(StoreError::InvalidInput("quota confidence must be finite and within 0..=1"));
	}
	if mutation.remaining_amount.is_some_and(|remaining| remaining < 0.0 || !remaining.is_finite())
	{
		return Err(StoreError::InvalidInput(
			"quota remaining amount must be finite and non-negative",
		));
	}
	if mutation.expected_revision.is_some_and(|revision| revision < 1) {
		return Err(StoreError::InvalidInput("expected revision must be positive"));
	}

	validate_rfc3339(&mutation.observed_at, "quota observed_at must be RFC 3339")?;

	if let Some(resets_at) = &mutation.resets_at {
		validate_rfc3339(resets_at, "quota resets_at must be RFC 3339")?;
	}

	crate::ensure_credential_negative_text(&mutation.window_class)?;
	crate::ensure_credential_negative_json(&mutation.metadata)?;

	Ok(())
}

fn validate_rfc3339(value: &str, error: &'static str) -> Result<(), StoreError> {
	if value.as_bytes().get(10) != Some(&b'T') {
		return Err(StoreError::InvalidInput(error));
	}

	OffsetDateTime::parse(value, &Rfc3339).map(|_| ()).map_err(|_| StoreError::InvalidInput(error))
}

fn account_from_row(row: Row) -> Result<AccountMetadata, StoreError> {
	Ok(AccountMetadata {
		account_id: AccountId::new(row.get::<_, String>(0))?,
		display_label: row.get(1),
		state: AccountState::from_sql(row.get::<_, &str>(2))?,
		metadata: row.get(3),
		revision: row.get(4),
	})
}

fn account_from_response(response: Value) -> Result<AccountMetadata, StoreError> {
	if response.get("kind").and_then(Value::as_str) != Some("account") {
		return Err(StoreError::IdempotencyConflict);
	}

	Ok(AccountMetadata {
		account_id: AccountId::new(required_str(&response, "account_id")?)?,
		display_label: required_str(&response, "display_label")?.to_owned(),
		state: AccountState::from_sql(required_str(&response, "state")?)?,
		metadata: response
			.get("metadata")
			.cloned()
			.ok_or(StoreError::Incompatible("account response missing metadata".into()))?,
		revision: required_i64(&response, "revision")?,
	})
}

fn window_from_response(response: Value) -> Result<QuotaWindow, StoreError> {
	if response.get("kind").and_then(Value::as_str) != Some("quota_window") {
		return Err(StoreError::IdempotencyConflict);
	}

	Ok(QuotaWindow {
		account_id: AccountId::new(required_str(&response, "account_id")?)?,
		window_class: required_str(&response, "window_class")?.to_owned(),
		duration_seconds: required_i64(&response, "duration_seconds")?,
		remaining_amount: response.get("remaining_amount").and_then(Value::as_f64),
		resets_at: response.get("resets_at").and_then(Value::as_str).map(str::to_owned),
		observed_at: required_str(&response, "observed_at")?.to_owned(),
		confidence: response
			.get("confidence")
			.and_then(Value::as_f64)
			.ok_or(StoreError::Incompatible("quota response missing confidence".into()))?,
		metadata: response
			.get("metadata")
			.cloned()
			.ok_or(StoreError::Incompatible("quota response missing metadata".into()))?,
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

async fn begin_command(
	transaction: &Transaction<'_>,
	command: &CommandIdentity,
) -> Result<Option<Value>, StoreError> {
	let inserted = transaction
		.query_opt(
			"INSERT INTO decodex.command_receipts (idempotency_key, request_hash) \
			 VALUES ($1, $2) ON CONFLICT DO NOTHING RETURNING true",
			&[&command.key, &command.request_hash],
		)
		.await?
		.is_some();

	if inserted {
		return Ok(None);
	}

	let row = transaction
		.query_one(
			"SELECT request_hash, response FROM decodex.command_receipts \
			 WHERE idempotency_key = $1 FOR UPDATE",
			&[&command.key],
		)
		.await?;
	let existing_hash: String = row.get(0);
	let response: Option<Value> = row.get(1);

	if existing_hash != command.request_hash {
		return Err(StoreError::IdempotencyConflict);
	}

	response.map_or_else(
		|| Err(StoreError::Incompatible("incomplete committed command receipt".into())),
		|response| Ok(Some(response)),
	)
}

async fn finish_command(
	transaction: &Transaction<'_>,
	command: &CommandIdentity,
	response: &Value,
) -> Result<(), StoreError> {
	let updated = transaction
		.execute(
			"UPDATE decodex.command_receipts SET response = $2, completed_at = clock_timestamp() \
			 WHERE idempotency_key = $1 AND request_hash = $3 AND response IS NULL",
			&[&command.key, response, &command.request_hash],
		)
		.await?;

	if updated == 1 { Ok(()) } else { Err(StoreError::IdempotencyConflict) }
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
				&mutation.state.as_sql(),
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
			 RETURNING revision",
			&[
				&mutation.account_id.as_str(),
				&mutation.display_label,
				&mutation.state.as_sql(),
				&mutation.metadata,
				&expected,
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

async fn create_window(
	transaction: &Transaction<'_>,
	mutation: &QuotaWindowMutation,
) -> Result<i64, StoreError> {
	let row = transaction
		.query_opt(
			"INSERT INTO decodex.quota_windows \
			 (account_id, window_class, duration_seconds, remaining_amount, resets_at, observed_at, confidence, metadata) \
			 VALUES ($1::text::uuid, $2, $3, $4, $5::text::timestamptz, $6::text::timestamptz, $7, $8) \
			 ON CONFLICT DO NOTHING RETURNING revision",
			&[
				&mutation.account_id.as_str(),
				&mutation.window_class,
				&mutation.duration_seconds,
				&mutation.remaining_amount,
				&mutation.resets_at,
				&mutation.observed_at,
				&mutation.confidence,
				&mutation.metadata,
			],
		)
		.await?;

	if let Some(row) = row {
		return Ok(row.get(0));
	}

	window_revision(transaction, mutation, None).await
}

async fn update_window(
	transaction: &Transaction<'_>,
	mutation: &QuotaWindowMutation,
	expected: i64,
) -> Result<i64, StoreError> {
	let row = transaction
		.query_opt(
			"UPDATE decodex.quota_windows SET remaining_amount = $4, resets_at = $5::timestamptz, \
			 observed_at = $6::timestamptz, confidence = $7, metadata = $8, revision = revision + 1, \
			 updated_at = clock_timestamp() WHERE account_id = $1::text::uuid AND window_class = $2 \
			 AND duration_seconds = $3 AND revision = $9 RETURNING revision",
			&[
				&mutation.account_id.as_str(),
				&mutation.window_class,
				&mutation.duration_seconds,
				&mutation.remaining_amount,
				&mutation.resets_at,
				&mutation.observed_at,
				&mutation.confidence,
				&mutation.metadata,
				&expected,
			],
		)
		.await?;

	if let Some(row) = row {
		return Ok(row.get(0));
	}

	window_revision(transaction, mutation, Some(expected)).await
}

async fn window_revision(
	transaction: &Transaction<'_>,
	mutation: &QuotaWindowMutation,
	expected: Option<i64>,
) -> Result<i64, StoreError> {
	let actual = transaction
		.query_opt(
			"SELECT revision FROM decodex.quota_windows WHERE account_id = $1::text::uuid \
			 AND window_class = $2 AND duration_seconds = $3",
			&[&mutation.account_id.as_str(), &mutation.window_class, &mutation.duration_seconds],
		)
		.await?
		.map(|row| row.get(0));

	Err(StoreError::RevisionConflict {
		entity: format!(
			"quota_window/{}:{}/{}",
			mutation.account_id, mutation.window_class, mutation.duration_seconds
		),
		expected,
		actual,
	})
}

async fn append_activity_and_outbox(
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
