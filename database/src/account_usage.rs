//! Atomic, revision-bound direct usage observations.

use decodex_core::{
	AccountId, AccountQuotaDisposition, AccountQuotaWindowObservation, AccountUsageObservation,
};
use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};

use crate::{SqliteStore, StoreError, error::sqlite_error};

pub(crate) fn read_usage_observation(
	connection: &Connection,
	account_id: &AccountId,
) -> Result<Option<AccountUsageObservation>, StoreError> {
	Ok(connection.query_row(
		"SELECT account_revision, observed_at_micros, ordinary_usage_allowed FROM account_usage_observations WHERE account_id=?1",
		[account_id.as_str()], |row| Ok(AccountUsageObservation {
			account_revision: row.get(0)?, observed_at_unix_micros: row.get(1)?, ordinary_usage_allowed: row.get(2)?,
		})
	).optional().map_err(sqlite_error)?)
}

impl SqliteStore {
	/// Commit permission and fresh windows together, only for the exact current account revision.
	/// Old responses cannot overwrite a new credential's observations.
	pub async fn observe_account_usage(
		&self,
		account_id: &AccountId,
		observation: AccountUsageObservation,
		windows: [Option<AccountQuotaWindowObservation>; 2],
	) -> Result<bool, StoreError> {
		if observation.account_revision <= 0 || observation.observed_at_unix_micros <= 0 {
			return Err(StoreError::InvalidInput("invalid usage observation"));
		}
		let account_id = account_id.clone();
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let current: bool = tx.query_row(
				"SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 AND revision=?2 AND tombstoned_at_micros IS NULL)",
				params![account_id.as_str(), observation.account_revision], |row| row.get(0)).map_err(sqlite_error)?;
			if !current { return Ok(false); }
			let prior = read_usage_observation(&tx, &account_id)?;
			if prior.is_some_and(|prior| prior.observed_at_unix_micros >= observation.observed_at_unix_micros) {
				return Ok(false);
			}
			if prior.is_none_or(|prior| prior.account_revision != observation.account_revision) {
				// A partial observation must not relabel the previous credential's windows.
				tx.execute("DELETE FROM account_quota_facts WHERE account_id=?1", [account_id.as_str()]).map_err(sqlite_error)?;
			}
			for (index, window) in windows.into_iter().enumerate() {
				if let Some(window) = window {
					write_window(&tx, &account_id, observation.observed_at_unix_micros, if index == 0 { 300 } else { 10080 }, window)?;
				}
			}
			tx.execute("INSERT INTO account_usage_observations(account_id,account_revision,observed_at_micros,ordinary_usage_allowed)
			 VALUES (?1,?2,?3,?4) ON CONFLICT(account_id) DO UPDATE SET account_revision=excluded.account_revision,
			 observed_at_micros=excluded.observed_at_micros,ordinary_usage_allowed=excluded.ordinary_usage_allowed",
			 params![account_id.as_str(), observation.account_revision, observation.observed_at_unix_micros, observation.ordinary_usage_allowed]).map_err(sqlite_error)?;
			tx.execute("UPDATE accounts SET state=CASE WHEN ?2=0 THEN 'depleted' WHEN ?2=1 THEN 'available'
			 WHEN EXISTS(SELECT 1 FROM account_quota_facts WHERE account_id=?1 AND error_code IS NULL AND used_percent>=100)
			 THEN 'depleted' ELSE 'available' END, updated_at_micros=?3 WHERE account_id=?1",
			 params![account_id.as_str(), observation.ordinary_usage_allowed, observation.observed_at_unix_micros]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(true)
		}).await
	}
}

fn write_window(
	connection: &Connection,
	account_id: &AccountId,
	now: i64,
	duration: u32,
	window: AccountQuotaWindowObservation,
) -> Result<(), StoreError> {
	if window.duration_minutes != duration || window.observed_at_unix_micros != Some(now) {
		return Err(StoreError::InvalidInput("invalid usage window"));
	}
	let (used, resets, absent) = match window.disposition {
		AccountQuotaDisposition::Current(fact)
			if fact.duration_minutes == duration && fact.resets_at_unix_micros > now =>
			(Some(i64::from(fact.used_percent)), Some(fact.resets_at_unix_micros), false),
		AccountQuotaDisposition::NotApplicable if duration == 300 => (None, None, true),
		_ => return Err(StoreError::InvalidInput("invalid usage window")),
	};
	let changed = connection.execute("INSERT INTO account_quota_facts(account_id,duration_minutes,used_percent,resets_at_micros,error_code,observed_at_micros,not_applicable)
	 VALUES (?1,?2,?3,?4,NULL,?5,?6) ON CONFLICT(account_id,duration_minutes) DO UPDATE SET used_percent=excluded.used_percent,
	 resets_at_micros=excluded.resets_at_micros,error_code=NULL,observed_at_micros=excluded.observed_at_micros,not_applicable=excluded.not_applicable
	 WHERE excluded.observed_at_micros >= account_quota_facts.observed_at_micros",
	 params![account_id.as_str(), i64::from(duration), used, resets, now, absent]).map_err(sqlite_error)?;
	if changed == 1 { Ok(()) } else { Err(StoreError::InvalidInput("older usage window")) }
}

#[cfg(test)]
mod tests {
	use super::{
		AccountId, AccountQuotaDisposition, AccountQuotaWindowObservation, AccountUsageObservation,
		SqliteStore, read_usage_observation,
	};
	use decodex_core::{AccountQuotaWindow, DecodexRoot};

	#[tokio::test]
	async fn permission_and_windows_survive_restart_and_reject_rotated_or_older_responses() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		let account = AccountId::new("10000000-0000-4000-8000-000000000001").unwrap();
		let id = account.clone();
		store.run(move |connection| {
			connection.execute("INSERT INTO account_identities VALUES (?1,1)", [id.as_str()]).unwrap();
			connection.execute("INSERT INTO accounts (account_id,display_label,enabled,state,revision,provider,provider_account_id,created_at_micros,updated_at_micros) VALUES (?1,'test',1,'available',1,'chatgpt','provider',1,1)", [id.as_str()]).unwrap();
			Ok(())
		}).await.unwrap();
		let observation = AccountUsageObservation {
			account_revision: 1,
			observed_at_unix_micros: 100,
			ordinary_usage_allowed: Some(false),
		};
		let window = AccountQuotaWindowObservation {
			duration_minutes: 10080,
			observed_at_unix_micros: Some(100),
			disposition: AccountQuotaDisposition::Current(
				AccountQuotaWindow::new(10080, 0, i64::MAX).unwrap(),
			),
		};
		assert!(
			store.observe_account_usage(&account, observation, [None, Some(window)]).await.unwrap()
		);
		let reopened = SqliteStore::open(&root.paths()).unwrap();
		let id = account.clone();
		reopened
			.run(move |connection| {
				assert_eq!(read_usage_observation(connection, &id)?, Some(observation));
				let state: String = connection
					.query_row("SELECT state FROM accounts", [], |row| row.get(0))
					.unwrap();
				assert_eq!(state, "depleted");
				let used: i64 = connection
					.query_row("SELECT used_percent FROM account_quota_facts", [], |row| row.get(0))
					.unwrap();
				assert_eq!(used, 0, "denial never fabricates a utilization percentage");
				connection.execute("UPDATE accounts SET revision=2", []).unwrap();
				Ok(())
			})
			.await
			.unwrap();
		assert!(
			!reopened
				.observe_account_usage(
					&account,
					AccountUsageObservation {
						observed_at_unix_micros: 200,
						ordinary_usage_allowed: Some(true),
						..observation
					},
					[None, None]
				)
				.await
				.unwrap()
		);
		let successor = AccountUsageObservation {
			account_revision: 2,
			observed_at_unix_micros: 300,
			ordinary_usage_allowed: None,
		};
		assert!(reopened.observe_account_usage(&account, successor, [None, None]).await.unwrap());
		assert!(
			!reopened
				.observe_account_usage(
					&account,
					AccountUsageObservation { observed_at_unix_micros: 299, ..successor },
					[None, None]
				)
				.await
				.unwrap()
		);
		let id = account.clone();
		reopened
			.run(move |connection| {
				assert_eq!(read_usage_observation(connection, &id)?, Some(successor));
				let count: i64 = connection
					.query_row("SELECT count(*) FROM account_quota_facts", [], |row| row.get(0))
					.unwrap();
				assert_eq!(count, 0, "a new revision cannot inherit the old credential's windows");
				Ok(())
			})
			.await
			.unwrap();
		let invalid = AccountQuotaWindowObservation { duration_minutes: 300, ..window };
		assert!(
			reopened
				.observe_account_usage(
					&account,
					AccountUsageObservation {
						observed_at_unix_micros: 400,
						ordinary_usage_allowed: Some(true),
						..successor
					},
					[Some(invalid), None]
				)
				.await
				.is_err()
		);
		reopened
			.run(move |connection| {
				assert_eq!(
					read_usage_observation(connection, &account)?,
					Some(successor),
					"invalid window rolls back the entire observation"
				);
				Ok(())
			})
			.await
			.unwrap();
	}
}
