//! Durable manual reset-card intents. An uncertain send is never automatically replayed.

use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};

use crate::{SqliteStore, StoreError, error::sqlite_error};

/// Private daemon ledger record. Do not log or expose the provider credit identifier.
#[derive(Clone, Eq, PartialEq)]
pub struct ResetCardOperation {
	pub key: String,
	pub account_id: String,
	pub account_revision: i64,
	pub granted_at: i64,
	pub expires_at: i64,
	pub exact_credit_id: Option<String>,
	pub state: String,
	pub outcome: Option<String>,
	pub failure: Option<String>,
}
impl SqliteStore {
	pub async fn reset_card_operation(
		&self,
		key: String,
	) -> Result<Option<ResetCardOperation>, StoreError> {
		self.run(move |db| read(db, &key)).await
	}

	/// Discover the latest operation after a client restart without a client-side journal.
	pub async fn latest_reset_card_operation(
		&self,
		account: String,
	) -> Result<Option<ResetCardOperation>, StoreError> {
		self.run(move |db| {
            let key: Option<String> = db.query_row(
                "SELECT idempotency_key FROM reset_card_operations WHERE account_id=?1 ORDER BY rowid DESC LIMIT 1",
                [account], |row| row.get(0),
            ).optional().map_err(sqlite_error)?;
            key.map(|key| read(db, &key)).transpose().map(Option::flatten)
        }).await
	}

	/// Persist an exact selection, or replay the original intent without rebinding its credit.
	pub async fn prepare_reset_card(
		&self,
		operation: ResetCardOperation,
	) -> Result<ResetCardOperation, StoreError> {
		if operation.key.is_empty()
			|| operation.key.len() > 256
			|| operation.key.chars().any(char::is_control)
			|| decodex_core::AccountId::new(operation.account_id.clone()).is_err()
			|| operation.account_revision <= 0
			|| operation.granted_at < 0
			|| operation.expires_at <= operation.granted_at
			|| operation.exact_credit_id.as_ref().is_none_or(|id| {
				id.is_empty() || id.len() > 1024 || id.chars().any(char::is_control)
			}) || operation.state != "prepared"
			|| operation.outcome.is_some()
			|| operation.failure.is_some()
		{
			return Err(StoreError::InvalidInput("reset-card intent"));
		}
		self.run(move |db| {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
            if let Some(old) = read(&tx, &operation.key)? {
                if old.account_id != operation.account_id || old.account_revision != operation.account_revision
                    || old.granted_at != operation.granted_at || old.expires_at != operation.expires_at {
                    return Err(StoreError::IdempotencyConflict);
                }
                return Ok(old);
            }
            let active: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM reset_card_operations
                 WHERE account_id=?1 AND state IN ('prepared','sending'))",
                [&operation.account_id], |row| row.get(0),
            ).map_err(sqlite_error)?;
            if active { return Err(StoreError::CapacityExhausted("account reset pending")); }
            tx.execute(
                "INSERT INTO reset_card_operations
                 (idempotency_key,account_id,account_revision,granted_at,expires_at,exact_credit_id,state)
                 VALUES(?1,?2,?3,?4,?5,?6,'prepared')",
                params![operation.key,operation.account_id,operation.account_revision,
                    operation.granted_at,operation.expires_at,operation.exact_credit_id],
            ).map_err(sqlite_error)?;
            tx.commit().map_err(sqlite_error)?;
            Ok(operation)
        }).await
	}

	/// Only never-sent intents and committed receipts can resume after a restart.
	pub async fn pending_reset_cards(&self) -> Result<Vec<ResetCardOperation>, StoreError> {
		self.run(|db| {
			let mut query = db
				.prepare(
					"SELECT idempotency_key FROM reset_card_operations
                 WHERE state='prepared' OR (state='sending' AND outcome IS NOT NULL) LIMIT 64",
				)
				.map_err(sqlite_error)?;
			let keys = query
				.query_map([], |row| row.get::<_, String>(0))
				.map_err(sqlite_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(sqlite_error)?;
			keys.into_iter()
				.map(|key| {
					read(db, &key)?.ok_or(StoreError::InvalidInput("reset-card intent missing"))
				})
				.collect()
		})
		.await
	}

	/// The durable no-retry barrier must commit before any provider write.
	pub async fn begin_reset_card_send(&self, key: String) -> Result<bool, StoreError> {
		self.run(move |db| {
			Ok(db
				.execute(
					"UPDATE reset_card_operations SET state='sending'
                 WHERE idempotency_key=?1 AND state='prepared'",
					[key],
				)
				.map_err(sqlite_error)?
				== 1)
		})
		.await
	}

	pub async fn record_reset_card_outcome(
		&self,
		key: String,
		outcome: String,
	) -> Result<(), StoreError> {
		self.run(move |db| {
			let changed = db
				.execute(
					"UPDATE reset_card_operations SET outcome=?2
                 WHERE idempotency_key=?1 AND state='sending' AND outcome IS NULL",
					params![key, outcome],
				)
				.map_err(sqlite_error)?;
			if changed != 1 {
				return Err(StoreError::OwnershipLost("reset-card receipt"));
			}
			Ok(())
		})
		.await
	}

	pub async fn complete_reset_card(&self, key: String) -> Result<(), StoreError> {
		self.run(move |db| {
			let changed = db
				.execute(
					"UPDATE reset_card_operations SET state='completed',exact_credit_id=NULL
                 WHERE idempotency_key=?1 AND state='sending' AND outcome IS NOT NULL",
					[key],
				)
				.map_err(sqlite_error)?;
			if changed != 1 {
				return Err(StoreError::OwnershipLost("reset-card completion"));
			}
			Ok(())
		})
		.await
	}

	pub async fn fail_reset_card_before_send(
		&self,
		key: String,
		failure: String,
	) -> Result<(), StoreError> {
		self.run(move |db| {
			let changed = db.execute(
                "UPDATE reset_card_operations SET state='failed',failure=?2,exact_credit_id=NULL
                 WHERE idempotency_key=?1 AND state='prepared'", params![key,failure],
            ).map_err(sqlite_error)?;
			if changed != 1 {
				return Err(StoreError::OwnershipLost("reset-card rejection"));
			}
			Ok(())
		})
		.await
	}
}
fn read(db: &Connection, key: &str) -> Result<Option<ResetCardOperation>, StoreError> {
	db.query_row(
		"SELECT idempotency_key,account_id,account_revision,granted_at,expires_at,
         exact_credit_id,state,outcome,failure FROM reset_card_operations WHERE idempotency_key=?1",
		[key],
		|row| {
			Ok(ResetCardOperation {
				key: row.get(0)?,
				account_id: row.get(1)?,
				account_revision: row.get(2)?,
				granted_at: row.get(3)?,
				expires_at: row.get(4)?,
				exact_credit_id: row.get(5)?,
				state: row.get(6)?,
				outcome: row.get(7)?,
				failure: row.get(8)?,
			})
		},
	)
	.optional()
	.map_err(sqlite_error)
	.map_err(Into::into)
}
