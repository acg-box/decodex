//! Daemon-owned persistent desktop presentation settings.

use rusqlite::{Connection, TransactionBehavior, params};

use crate::{SqliteStore, StoreError, error::sqlite_error};

/// Complete persistent desktop settings projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopSettings {
	/// Whether the sole Decodex application exposes its in-process menu-bar item.
	pub show_in_menu_bar: bool,
	/// Positive optimistic revision of this singleton projection.
	pub revision: i64,
}

impl SqliteStore {
	/// Read the complete daemon-owned desktop settings projection.
	pub async fn read_desktop_settings(&self) -> Result<DesktopSettings, StoreError> {
		self.run(|connection| read_desktop_settings(connection)).await
	}

	/// Replace the menu-bar preference under one exact optimistic revision.
	pub async fn set_show_in_menu_bar(
		&self,
		expected_revision: i64,
		show_in_menu_bar: bool,
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
					 SET show_in_menu_bar = ?1, revision = ?2
					 WHERE singleton = 1 AND revision = ?3",
					params![show_in_menu_bar, revision, expected_revision],
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
			Ok(DesktopSettings { show_in_menu_bar, revision })
		})
		.await
	}
}

fn read_desktop_settings(connection: &Connection) -> Result<DesktopSettings, StoreError> {
	let (show_in_menu_bar, revision) = connection
		.query_row(
			"SELECT show_in_menu_bar, revision FROM desktop_settings WHERE singleton = 1",
			[],
			|row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
		)
		.map_err(sqlite_error)?;
	if !matches!(show_in_menu_bar, 0 | 1) || revision <= 0 {
		return Err(StoreError::Incompatible("desktop_settings".to_owned()));
	}
	Ok(DesktopSettings { show_in_menu_bar: show_in_menu_bar == 1, revision })
}

#[cfg(test)]
mod tests {
	use tempfile::tempdir;

	use crate::{SqliteStore, StoreError};

	#[tokio::test]
	async fn desktop_menu_bar_preference_is_revision_guarded_and_survives_reopen() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");

		let initial = store.read_desktop_settings().await.expect("read default settings");
		assert!(initial.show_in_menu_bar);
		assert_eq!(initial.revision, 1);

		let changed = store
			.set_show_in_menu_bar(initial.revision, false)
			.await
			.expect("persist disabled menu bar");
		assert_eq!(changed.revision, 2);
		assert!(!changed.show_in_menu_bar);
		assert!(matches!(
			store.set_show_in_menu_bar(initial.revision, true).await,
			Err(StoreError::RevisionConflict { expected: Some(1), actual: Some(2), .. })
		));

		drop(store);
		let reopened = SqliteStore::open_test(&path).expect("reopen store");
		assert_eq!(
			reopened.read_desktop_settings().await.expect("read persisted settings"),
			changed
		);
	}
}
