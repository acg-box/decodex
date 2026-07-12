use crate::state::sqlite_store::schema::{self, OptionalExtension, Result, SqliteStateStore};

impl SqliteStateStore {
	pub(in crate::state) fn reject_future_schema_version(&self) -> Result<()> {
		let has_meta = self.connection.query_row(
			"SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_meta')",
			[],
			|row| row.get::<_, bool>(0),
		)?;
		if !has_meta {
			return Ok(());
		}
		if self.schema_version()?.is_some_and(|version| version > 16) {
			crate::prelude::eyre::bail!("Runtime schema was created by a newer Decodex binary.");
		}
		Ok(())
	}

	pub(in crate::state) fn schema_version(&self) -> Result<Option<i64>> {
		self.ensure_schema_meta_table()?;

		let version = self
			.connection
			.query_row("SELECT value FROM schema_meta WHERE key = 'schema_version'", [], |row| {
				row.get::<_, String>(0)
			})
			.optional()?
			.and_then(|value| value.parse::<i64>().ok());

		Ok(version)
	}

	pub(in crate::state) fn ensure_schema_meta_table(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS schema_meta (
	key TEXT PRIMARY KEY NOT NULL,
	value TEXT NOT NULL
);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn schema_migration_completed(&self, key: &str) -> Result<bool> {
		self.ensure_schema_meta_table()?;

		let value = self
			.connection
			.query_row(
				"SELECT value FROM schema_meta WHERE key = ?1",
				schema::params![key],
				|row| row.get::<_, String>(0),
			)
			.optional()?;

		Ok(value.as_deref() == Some("completed"))
	}

	pub(in crate::state) fn record_schema_migration_completed(&self, key: &str) -> Result<()> {
		self.ensure_schema_meta_table()?;
		self.connection.execute(
			"INSERT INTO schema_meta (key, value)
			 VALUES (?1, 'completed')
			 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
			schema::params![key],
		)?;

		Ok(())
	}

	pub(in crate::state) fn run_schema_migrations(&self) -> Result<()> {
		let version = self.schema_version()?.unwrap_or(0);

		if version > 16 {
			crate::prelude::eyre::bail!("Runtime schema was created by a newer Decodex binary.");
		}

		if version < 12
			&& !self
				.schema_migration_completed("migration:protocol_event_summaries_from_events:v12")?
		{
			self.backfill_protocol_event_summaries_from_events()?;
			self.record_schema_migration_completed(
				"migration:protocol_event_summaries_from_events:v12",
			)?;
		}

		self.migrate_removed_decision_contract_fields()?;

		Ok(())
	}

	pub(in crate::state) fn record_schema_version(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS schema_meta (
	key TEXT PRIMARY KEY NOT NULL,
	value TEXT NOT NULL
);
INSERT INTO schema_meta (key, value)
VALUES ('schema_version', '16')
ON CONFLICT(key) DO UPDATE SET value =
	CASE
		WHEN CAST(schema_meta.value AS INTEGER) < CAST(excluded.value AS INTEGER)
		THEN excluded.value
		ELSE schema_meta.value
	END;
"#,
		)?;

		Ok(())
	}
}
