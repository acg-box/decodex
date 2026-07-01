#[allow(clippy::wildcard_imports)]
use super::*;

impl SqliteStateStore {
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
			.query_row("SELECT value FROM schema_meta WHERE key = ?1", params![key], |row| {
				row.get::<_, String>(0)
			})
			.optional()?;

		Ok(value.as_deref() == Some("completed"))
	}

	pub(in crate::state) fn record_schema_migration_completed(&self, key: &str) -> Result<()> {
		self.ensure_schema_meta_table()?;
		self.connection.execute(
			"INSERT INTO schema_meta (key, value)
			 VALUES (?1, 'completed')
			 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
			params![key],
		)?;

		Ok(())
	}

	pub(in crate::state) fn run_schema_migrations(&self) -> Result<()> {
		let version = self.schema_version()?.unwrap_or(0);

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
VALUES ('schema_version', '12')
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

	pub(in crate::state) fn backfill_protocol_event_summaries_from_events(&self) -> Result<()> {
		let now = timestamp_parts();

		self.connection.execute(
			"INSERT INTO protocol_event_summaries (
					run_id, event_count, last_sequence_number, last_event_type, last_event_at,
					last_event_at_unix, compacted_at, compacted_at_unix
				)
			 SELECT totals.run_id, totals.event_count, totals.last_sequence_number,
					last.event_type, last.created_at, last.created_at_unix, ?1, ?2
			 FROM (
				 SELECT run_id, COUNT(*) AS event_count, MAX(sequence_number) AS last_sequence_number
				 FROM protocol_events
				 GROUP BY run_id
			 ) totals
			 JOIN protocol_events last
			 ON last.run_id = totals.run_id
			 AND last.sequence_number = totals.last_sequence_number
			 ON CONFLICT(run_id) DO UPDATE SET
				 event_count = excluded.event_count,
				 last_sequence_number = excluded.last_sequence_number,
				 last_event_type = excluded.last_event_type,
				 last_event_at = excluded.last_event_at,
				 last_event_at_unix = excluded.last_event_at_unix,
				 compacted_at = excluded.compacted_at,
				 compacted_at_unix = excluded.compacted_at_unix",
			params![now.text, now.unix],
		)?;

		Ok(())
	}

	pub(in crate::state) fn migrate_removed_decision_contract_fields(&self) -> Result<()> {
		let updates = {
			let mut statement = self.connection.prepare(
				"SELECT project_id, contract_id, payload_json
				 FROM decision_contracts
				 WHERE json_type(payload_json, '$.execution_readiness.proposed_issue_summaries') IS NOT NULL
				 OR json_type(payload_json, '$.execution_readiness.queue_intent') IS NOT NULL
				 ORDER BY project_id ASC, contract_id ASC",
			)?;
			let rows = statement.query_map([], |row| {
				Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
			})?;
			let mut updates = Vec::new();

			for row in rows {
				let (project_id, contract_id, payload_json) = row?;
				let migrated_payload = migrate_removed_decision_contract_fields(&payload_json)
					.map_err(|error| {
						eyre::eyre!(
							"Decision Contract `{project_id}/{contract_id}` removed-field migration failed: {error}"
						)
					})?;

				if migrated_payload != payload_json {
					updates.push((project_id, contract_id, migrated_payload));
				}
			}

			updates
		};

		for (project_id, contract_id, payload_json) in updates {
			self.connection.execute(
				"UPDATE decision_contracts
				 SET payload_json = ?3
				 WHERE project_id = ?1 AND contract_id = ?2",
				params![project_id, contract_id, payload_json],
			)?;
		}

		Ok(())
	}

	pub(in crate::state) fn seal_run_activity_summary_records(&self) -> Result<()> {
		let updates = {
			let mut statement = self.connection.prepare(
				"SELECT run_id, child_agent_activity_json FROM run_activity_summaries \
				 WHERE child_agent_activity_json IS NOT NULL",
			)?;
			let rows = statement
				.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
			let mut updates = Vec::new();

			for row in rows {
				let (run_id, child_agent_activity_json) = row?;
				let sealed_json = serde_json::to_string(
					&serde_json::from_str::<ChildAgentActivitySummary>(&child_agent_activity_json)?
						.sealed_durable(),
				)?;

				if sealed_json != child_agent_activity_json {
					updates.push((run_id, sealed_json));
				}
			}

			updates
		};

		for (run_id, child_agent_activity_json) in updates {
			self.connection.execute(
				"UPDATE run_activity_summaries SET child_agent_activity_json = ?2 WHERE run_id = ?1",
				params![run_id, child_agent_activity_json],
			)?;
		}

		Ok(())
	}
}
