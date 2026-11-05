//! Daemon-owned quota activation preference.

use rusqlite::{self, Connection, TransactionBehavior};

use crate::{SqliteStore, StoreError, error::sqlite_error};

/// Complete persistent desktop settings projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopSettings {
	/// Whether each account receives a minimal request after its weekly reset.
	pub auto_activate_quota: bool,
	/// Positive optimistic revision of this singleton projection.
	pub revision: i64,
}

impl SqliteStore {
	/// Read the complete daemon-owned desktop settings projection.
	pub async fn read_desktop_settings(&self) -> Result<DesktopSettings, StoreError> {
		self.run(|connection| read_desktop_settings(connection)).await
	}

	/// Update quota activation under one exact optimistic revision.
	pub async fn set_desktop_settings(
		&self,
		expected_revision: i64,
		auto_activate_quota: bool,
	) -> Result<DesktopSettings, StoreError> {
		if expected_revision <= 0 {
			return Err(StoreError::InvalidInput("desktop settings revision must be positive"));
		}

		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sqlite_error)?;
			let current = read_desktop_settings(&transaction)?;

			if current.revision != expected_revision {
				return Err(StoreError::RevisionConflict {
					entity: "desktop_settings".to_owned(),
					expected: Some(expected_revision),
					actual: Some(current.revision),
				});
			}

			let revision = current
				.revision
				.checked_add(1)
				.ok_or(StoreError::CapacityExhausted("desktop settings revision"))?;
			let changed = transaction
				.execute(
					"UPDATE desktop_settings
					 SET auto_activate_quota = ?1, revision = ?2
					 WHERE singleton = 1 AND revision = ?3",
					rusqlite::params![auto_activate_quota, revision, expected_revision],
				)
				.map_err(sqlite_error)?;

			if changed != 1 {
				return Err(StoreError::RevisionConflict {
					entity: "desktop_settings".to_owned(),
					expected: Some(expected_revision),
					actual: Some(current.revision),
				});
			}

			transaction.commit().map_err(sqlite_error)?;

			Ok(DesktopSettings { auto_activate_quota, revision })
		})
		.await
	}
}

fn read_desktop_settings(connection: &Connection) -> Result<DesktopSettings, StoreError> {
	Ok(connection
		.query_row(
			"SELECT auto_activate_quota, revision FROM desktop_settings WHERE singleton = 1",
			[],
			|row| Ok(DesktopSettings { auto_activate_quota: row.get(0)?, revision: row.get(1)? }),
		)
		.map_err(sqlite_error)?)
}

#[cfg(test)]
mod tests {
	use crate::{SqliteStore, StoreError};

	#[tokio::test]
	async fn quota_activation_is_revision_guarded_and_survives_reopen() {
		let directory = tempfile::tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");
		let initial = store.read_desktop_settings().await.expect("default settings");
		assert!(initial.auto_activate_quota);
		let changed =
			store.set_desktop_settings(initial.revision, false).await.expect("disable activation");
		assert_eq!(changed.revision, initial.revision + 1);
		assert!(!changed.auto_activate_quota);
		assert!(matches!(
			store.set_desktop_settings(initial.revision, true).await,
			Err(StoreError::RevisionConflict { expected: Some(1), actual: Some(2), .. })
		));
		drop(store);
		let reopened = SqliteStore::open_test(&path).expect("reopen store");
		assert_eq!(reopened.read_desktop_settings().await.expect("persisted settings"), changed);
	}
}
