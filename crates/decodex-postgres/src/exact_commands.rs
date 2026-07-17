use tokio_postgres::{Row, error::SqlState, types::ToSql};

use crate::{PostgresStore, StoreError};

pub(crate) const EXACT_COMMAND_PROTOCOL: &str = "decodex/exact-command/1";
const MAX_EXACT_ATTEMPTS: usize = 4;

impl PostgresStore {
	pub(crate) async fn execute_exact_with_retry(
		&self,
		statement: &str,
		parameters: &[&(dyn ToSql + Sync)],
	) -> Result<Vec<u8>, StoreError> {
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
			let response = response_bytes(row)?;

			match transaction.commit().await {
				Ok(()) => return Ok(response),
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

fn is_retryable_exact_database_error(error: &tokio_postgres::Error) -> bool {
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
