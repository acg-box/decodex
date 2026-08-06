use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tokio_postgres::{Row, error::SqlState, types::ToSql};

use crate::{PostgresStore, StoreError};

pub(crate) const EXACT_COMMAND_PROTOCOL: &str = "decodex/exact-command/1";
pub(crate) const MAX_EXACT_ATTEMPTS: usize = 4;

impl PostgresStore {
	pub(crate) async fn execute_exact_with_retry(
		&self,
		statement: &str,
		parameters: &[&(dyn ToSql + Sync)],
	) -> Result<Vec<u8>, StoreError> {
		let row = self.execute_exact_row_with_retry(statement, parameters).await?;

		response_bytes(row)
	}

	pub(crate) async fn execute_exact_with_replay_status(
		&self,
		statement: &str,
		parameters: &[&(dyn ToSql + Sync)],
	) -> Result<(Vec<u8>, bool), StoreError> {
		let row = self.execute_exact_row_with_retry(statement, parameters).await?;
		let replayed: bool = row.get(1);

		Ok((response_bytes(row)?, replayed))
	}

	async fn execute_exact_row_with_retry(
		&self,
		statement: &str,
		parameters: &[&(dyn ToSql + Sync)],
	) -> Result<Row, StoreError> {
		let mut last_retryable = None;

		for _ in 0..MAX_EXACT_ATTEMPTS {
			let mut client = match self.pool().get().await {
				Ok(client) => client,
				Err(error) => return Err(StoreError::Pool(error)),
			};
			let transaction = match client.transaction().await {
				Ok(transaction) => transaction,
				Err(error) if is_retryable_exact_database_error(&error) => {
					last_retryable = Some(error);
					continue;
				},
				Err(error) => return Err(StoreError::from(error)),
			};
			let row = match transaction.query_one(statement, parameters).await {
				Ok(row) => row,
				Err(error) if is_retryable_exact_database_error(&error) => {
					last_retryable = Some(error);
					continue;
				},
				Err(error) => return Err(StoreError::from(error)),
			};
			match transaction.commit().await {
				Ok(()) => return Ok(row),
				Err(error) if is_retryable_exact_database_error(&error) => {
					last_retryable = Some(error);
				},
				Err(error) => return Err(StoreError::from(error)),
			}
		}

		Err(StoreError::Database(
			last_retryable.expect(
				"an exhausted exact retry loop retains its classified infrastructure failure",
			),
		))
	}
}

fn response_bytes(row: Row) -> Result<Vec<u8>, StoreError> {
	let response: Option<Vec<u8>> = row.get(0);

	response.ok_or_else(|| StoreError::Incompatible("exact command response is null".into()))
}

pub(crate) fn validate_exact_key(key: &str) -> Result<(), StoreError> {
	if key.is_empty() || key.len() > 256 {
		return Err(StoreError::InvalidInput("idempotency key must contain 1..=256 bytes"));
	}

	crate::ensure_credential_negative_text(key)
}

pub(crate) fn validate_exact_effect_digest(effect: &Value) -> Result<(), StoreError> {
	let digest = effect
		.get("effect_digest")
		.and_then(Value::as_str)
		.ok_or_else(|| StoreError::Incompatible("exact command effect digest is absent".into()))?;
	let source = effect.get("effect_digest_source").and_then(Value::as_str).ok_or_else(|| {
		StoreError::Incompatible("exact command effect digest source is absent".into())
	})?;
	if digest.len() != 64
		|| !digest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
	{
		return Err(StoreError::Incompatible("exact command effect digest is invalid".into()));
	}
	let actual = Sha256::digest(source.as_bytes())
		.iter()
		.map(|byte| format!("{byte:02x}"))
		.collect::<String>();
	if actual != digest {
		return Err(StoreError::Incompatible(
			"exact command effect digest does not match its source".into(),
		));
	}
	let source_value: Value = serde_json::from_str(source).map_err(|_| {
		StoreError::Incompatible("exact command effect digest source is invalid".into())
	})?;
	let mut projection = effect
		.as_object()
		.cloned()
		.ok_or_else(|| StoreError::Incompatible("exact command effect is not an object".into()))?;
	projection.remove("effect_digest");
	projection.remove("effect_digest_source");
	if source_value != Value::Object(projection) {
		return Err(StoreError::Incompatible(
			"exact command effect differs from its digest source".into(),
		));
	}
	Ok(())
}

pub(crate) fn is_retryable_exact_database_error(error: &tokio_postgres::Error) -> bool {
	let Some(code) = error.code() else {
		return false;
	};

	code == &SqlState::T_R_SERIALIZATION_FAILURE
		|| code == &SqlState::T_R_DEADLOCK_DETECTED
		|| code == &SqlState::ADMIN_SHUTDOWN
		|| code == &SqlState::CRASH_SHUTDOWN
		|| code == &SqlState::CANNOT_CONNECT_NOW
		|| code.code().starts_with("08")
}
