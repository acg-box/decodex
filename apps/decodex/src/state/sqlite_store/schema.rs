use super::{
	ChildAgentActivitySummary, OptionalExtension, Result, SqliteStateStore,
	execution_program_record_from_row_parts, execution_program_runtime_row_parts, eyre,
	migrate_removed_decision_contract_fields, params, timestamp_parts,
};

const REVIEW_LIFECYCLE_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS review_lifecycle_records (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	branch_name TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	pr_url TEXT NOT NULL,
	target_base_ref_name TEXT,
	pr_head_ref_name TEXT NOT NULL,
	pr_head_oid TEXT NOT NULL,
	head_sha TEXT NOT NULL,
	phase TEXT NOT NULL,
	request_comment_database_id INTEGER,
	request_created_at_unix_epoch INTEGER,
	request_description_thumbs_up_count INTEGER,
	request_retry_count INTEGER NOT NULL,
	external_round_count INTEGER NOT NULL,
	auto_merge_enabled_at_unix_epoch INTEGER,
	landing_state TEXT NOT NULL DEFAULT 'not_started',
	closeout_state TEXT NOT NULL DEFAULT 'not_started',
	repair_attempt_count INTEGER NOT NULL DEFAULT 0,
	evidence_json TEXT NOT NULL DEFAULT '{}',
	next_action TEXT NOT NULL DEFAULT '',
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, branch_name)
);
CREATE TABLE IF NOT EXISTS review_policy_checkpoints (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	phase TEXT NOT NULL,
	status TEXT NOT NULL,
	head_sha TEXT NOT NULL,
	nonclean_rounds INTEGER NOT NULL,
	details_json TEXT NOT NULL DEFAULT '{}',
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, run_id, attempt_number, phase)
);
"#;
const EVIDENCE_ARTIFACT_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS evidence_artifacts (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	artifact_kind TEXT NOT NULL,
	key_hash TEXT NOT NULL,
	phase TEXT NOT NULL,
	status TEXT NOT NULL,
	head_sha TEXT,
	key_json TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	source_run_id TEXT NOT NULL,
	source_attempt_number INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, artifact_kind, key_hash)
);
CREATE INDEX IF NOT EXISTS evidence_artifacts_lookup_idx
ON evidence_artifacts (project_id, issue_id, artifact_kind, phase, head_sha, status);
"#;
const DROP_LEGACY_REVIEW_MARKER_TABLES_SQL: &str = r#"
DROP TABLE IF EXISTS review_handoffs;
DROP TABLE IF EXISTS review_orchestrations;
"#;

impl SqliteStateStore {
	pub(in crate::state) fn bootstrap_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS projects (
	service_id TEXT PRIMARY KEY NOT NULL,
	config_path TEXT NOT NULL,
	repo_root TEXT NOT NULL,
	worktree_root TEXT NOT NULL,
	workflow_path TEXT NOT NULL,
	tracker_api_key_env_var TEXT NOT NULL,
	github_token_env_var TEXT NOT NULL,
	enabled INTEGER NOT NULL,
	config_fingerprint TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS leases (
	issue_id TEXT PRIMARY KEY NOT NULL,
	project_id TEXT NOT NULL,
	run_id TEXT NOT NULL,
	issue_state TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS run_attempts (
	run_id TEXT PRIMARY KEY NOT NULL,
	project_id TEXT,
	issue_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	status TEXT NOT NULL,
	thread_id TEXT,
	turn_id TEXT,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS run_attempts_issue_attempt_idx
ON run_attempts (issue_id, attempt_number, updated_at_unix, run_id);
CREATE TABLE IF NOT EXISTS protocol_events (
	run_id TEXT NOT NULL,
	sequence_number INTEGER NOT NULL,
	event_type TEXT NOT NULL,
	payload_sha256 TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	PRIMARY KEY (run_id, sequence_number)
);
CREATE TABLE IF NOT EXISTS protocol_event_summaries (
	run_id TEXT PRIMARY KEY NOT NULL,
	event_count INTEGER NOT NULL,
	last_sequence_number INTEGER,
	last_event_type TEXT,
	last_event_at TEXT,
	last_event_at_unix INTEGER,
	compacted_at TEXT NOT NULL,
	compacted_at_unix INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS run_activity_summaries (
	run_id TEXT PRIMARY KEY NOT NULL,
	attempt_number INTEGER NOT NULL,
	child_agent_activity_json TEXT,
	protocol_activity_json TEXT,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS worktrees (
	issue_id TEXT PRIMARY KEY NOT NULL,
	project_id TEXT NOT NULL,
	branch_name TEXT NOT NULL,
	worktree_path TEXT NOT NULL,
	provenance_source TEXT NOT NULL DEFAULT 'runtime_recorded',
	created_at_unix INTEGER,
	updated_at_unix INTEGER
);
CREATE INDEX IF NOT EXISTS worktrees_project_issue_idx
ON worktrees (project_id, issue_id);
CREATE TABLE IF NOT EXISTS linear_execution_events (
	idempotency_key TEXT PRIMARY KEY NOT NULL,
	service_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	event_type TEXT NOT NULL,
	event_timestamp TEXT NOT NULL,
	event_unix INTEGER,
	payload_json TEXT NOT NULL,
	recorded_at TEXT NOT NULL,
	recorded_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS linear_execution_events_issue_idx
ON linear_execution_events (service_id, issue_id, event_unix, recorded_at_unix);
"#,
		)?;
		self.bootstrap_worktree_schema()?;
		self.bootstrap_review_schema()?;
		self.bootstrap_evidence_artifact_schema()?;
		self.bootstrap_run_control_channels_schema()?;
		self.bootstrap_connector_backoffs_schema()?;
		self.bootstrap_private_execution_events_schema()?;
		self.bootstrap_decision_contracts_schema()?;
		self.bootstrap_autonomy_objectives_schema()?;
		self.bootstrap_autonomy_signals_schema()?;
		self.bootstrap_autonomy_proposals_schema()?;
		self.bootstrap_execution_programs_schema()?;
		self.bootstrap_program_intake_state_schema()?;
		self.bootstrap_loop_guardrail_schema()?;
		self.run_schema_migrations()?;
		self.record_schema_version()?;
		self.seal_run_activity_summary_records()?;
		self.connection.execute_batch("PRAGMA optimize=0x10002;")?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_worktree_schema(&self) -> Result<()> {
		self.ensure_column(
			"worktrees",
			"provenance_source",
			"ALTER TABLE worktrees ADD COLUMN provenance_source TEXT NOT NULL DEFAULT 'legacy_unknown'",
		)?;
		self.ensure_column(
			"worktrees",
			"created_at_unix",
			"ALTER TABLE worktrees ADD COLUMN created_at_unix INTEGER",
		)?;
		self.ensure_column(
			"worktrees",
			"updated_at_unix",
			"ALTER TABLE worktrees ADD COLUMN updated_at_unix INTEGER",
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_review_schema(&self) -> Result<()> {
		self.connection.execute_batch(DROP_LEGACY_REVIEW_MARKER_TABLES_SQL)?;
		self.connection.execute_batch(REVIEW_LIFECYCLE_SCHEMA_SQL)?;
		self.ensure_column(
			"review_policy_checkpoints",
			"details_json",
			"ALTER TABLE review_policy_checkpoints ADD COLUMN details_json TEXT NOT NULL DEFAULT '{}'",
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_evidence_artifact_schema(&self) -> Result<()> {
		self.connection.execute_batch(EVIDENCE_ARTIFACT_SCHEMA_SQL)?;

		Ok(())
	}

	pub(in crate::state) fn ensure_column(
		&self,
		table: &str,
		column: &str,
		add_column_sql: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(&format!("PRAGMA table_info({table})"))?;
		let column_names = statement.query_map([], |row| row.get::<_, String>(1))?;

		for column_name in column_names {
			if column_name? == column {
				return Ok(());
			}
		}

		self.connection.execute_batch(add_column_sql)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_run_control_channels_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS run_control_channels (
	run_id TEXT PRIMARY KEY NOT NULL,
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	transport TEXT NOT NULL,
	channel_path TEXT NOT NULL,
	status TEXT NOT NULL,
	published_at TEXT NOT NULL,
	published_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS run_control_channels_project_issue_idx
ON run_control_channels (project_id, issue_id, attempt_number);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_loop_guardrail_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS loop_guardrail_checkpoints (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	reason TEXT NOT NULL,
	fingerprint TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	consecutive_count INTEGER NOT NULL,
	details_json TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, reason)
);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_connector_backoffs_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS connector_backoffs (
	project_id TEXT NOT NULL,
	connector TEXT NOT NULL,
	sync_phase TEXT NOT NULL,
	quota_class TEXT NOT NULL,
	reset_unix_epoch INTEGER NOT NULL,
	reset_source TEXT NOT NULL,
	warning TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, connector)
);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_private_execution_events_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS private_execution_events (
	record_id INTEGER PRIMARY KEY AUTOINCREMENT,
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	event_type TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	recorded_at TEXT NOT NULL,
	recorded_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS private_execution_events_attempt_idx
ON private_execution_events (
	project_id, issue_id, run_id, attempt_number, record_id
);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_decision_contracts_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS decision_contracts (
	project_id TEXT NOT NULL,
	contract_id TEXT NOT NULL,
	source_issue_id TEXT,
	status TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, contract_id)
);
CREATE INDEX IF NOT EXISTS decision_contracts_source_issue_idx
ON decision_contracts (project_id, source_issue_id, updated_at_unix);
CREATE INDEX IF NOT EXISTS decision_contracts_status_idx
ON decision_contracts (project_id, status, updated_at_unix);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_autonomy_objectives_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS autonomy_objectives (
	project_id TEXT NOT NULL,
	objective_id TEXT NOT NULL,
	version INTEGER NOT NULL,
	state TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, objective_id, version)
);
CREATE INDEX IF NOT EXISTS autonomy_objectives_project_state_idx
ON autonomy_objectives (project_id, state, updated_at_unix);
CREATE INDEX IF NOT EXISTS autonomy_objectives_history_idx
ON autonomy_objectives (project_id, objective_id, version);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_autonomy_signals_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS autonomy_signals (
	project_id TEXT NOT NULL,
	signal_id TEXT NOT NULL,
	objective_id TEXT NOT NULL,
	objective_version INTEGER NOT NULL,
	kind TEXT NOT NULL,
	fingerprint TEXT NOT NULL,
	freshness TEXT NOT NULL,
	evidence_class TEXT NOT NULL,
	confidence TEXT NOT NULL,
	privacy TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, signal_id)
);
CREATE INDEX IF NOT EXISTS autonomy_signals_objective_idx
ON autonomy_signals (project_id, objective_id, objective_version, updated_at_unix);
CREATE INDEX IF NOT EXISTS autonomy_signals_recent_idx
ON autonomy_signals (project_id, updated_at_unix);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_autonomy_proposals_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS autonomy_proposals (
	project_id TEXT NOT NULL,
	proposal_id TEXT NOT NULL,
	objective_id TEXT NOT NULL,
	objective_version INTEGER NOT NULL,
	state TEXT NOT NULL,
	fingerprint TEXT NOT NULL,
	source_family TEXT NOT NULL,
	intended_surface TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, proposal_id)
);
CREATE INDEX IF NOT EXISTS autonomy_proposals_objective_idx
ON autonomy_proposals (project_id, objective_id, objective_version, updated_at_unix);
CREATE INDEX IF NOT EXISTS autonomy_proposals_state_idx
ON autonomy_proposals (project_id, state, updated_at_unix);
CREATE INDEX IF NOT EXISTS autonomy_proposals_recent_idx
ON autonomy_proposals (project_id, updated_at_unix);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_execution_programs_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS execution_programs (
	project_id TEXT NOT NULL,
	program_id TEXT NOT NULL,
	source_contract_id TEXT,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, program_id)
);
CREATE INDEX IF NOT EXISTS execution_programs_source_contract_idx
ON execution_programs (project_id, source_contract_id, updated_at_unix);
"#,
		)?;
		self.ensure_execution_program_source_contract_nullable()?;

		Ok(())
	}

	pub(in crate::state) fn ensure_execution_program_source_contract_nullable(&self) -> Result<()> {
		let mut statement = self.connection.prepare("PRAGMA table_info(execution_programs)")?;
		let columns =
			statement.query_map([], |row| Ok((row.get::<_, String>(1)?, row.get::<_, i64>(3)?)))?;
		let mut source_contract_not_null = false;

		for column in columns {
			let (name, not_null) = column?;

			if name == "source_contract_id" {
				source_contract_not_null = not_null != 0;

				break;
			}
		}

		if !source_contract_not_null {
			return Ok(());
		}

		self.connection.execute_batch(
			r#"
ALTER TABLE execution_programs RENAME TO execution_programs_legacy_contract_required;
CREATE TABLE execution_programs (
	project_id TEXT NOT NULL,
	program_id TEXT NOT NULL,
	source_contract_id TEXT,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, program_id)
);
INSERT INTO execution_programs (
	project_id, program_id, source_contract_id, payload_json, created_at, created_at_unix,
	updated_at, updated_at_unix
)
SELECT project_id, program_id, source_contract_id, payload_json, created_at, created_at_unix,
	updated_at, updated_at_unix
FROM execution_programs_legacy_contract_required;
DROP TABLE execution_programs_legacy_contract_required;
CREATE INDEX IF NOT EXISTS execution_programs_source_contract_idx
ON execution_programs (project_id, source_contract_id, updated_at_unix);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_program_intake_state_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS program_intake_plans (
	project_id TEXT NOT NULL,
	program_id TEXT NOT NULL,
	plan_id TEXT NOT NULL,
	intake_kind TEXT NOT NULL,
	source_contract_id TEXT,
	accepted_contract_fingerprint TEXT NOT NULL,
	public_summary TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, program_id, plan_id)
);
CREATE INDEX IF NOT EXISTS program_intake_plans_project_idx
ON program_intake_plans (project_id, intake_kind, updated_at_unix);
DROP TABLE IF EXISTS program_issue_mappings;
DROP TABLE IF EXISTS program_queue_label_ownership;
CREATE TABLE IF NOT EXISTS program_issue_mappings (
	project_id TEXT NOT NULL,
	program_id TEXT NOT NULL,
	node_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	issue_identifier TEXT NOT NULL,
	issue_state TEXT NOT NULL,
	queue_intent TEXT NOT NULL,
	has_active_label INTEGER NOT NULL,
	has_opt_out_label INTEGER NOT NULL,
	has_needs_attention_label INTEGER NOT NULL,
	has_generic_dispatch_briefing INTEGER NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, program_id, node_id)
);
CREATE INDEX IF NOT EXISTS program_issue_mappings_issue_idx
ON program_issue_mappings (project_id, issue_id, updated_at_unix);
"#,
		)?;
		self.backfill_program_intake_state_from_execution_programs()?;

		Ok(())
	}

	pub(in crate::state) fn backfill_program_intake_state_from_execution_programs(
		&self,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, source_contract_id, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM execution_programs \
			 ORDER BY project_id ASC, program_id ASC",
		)?;
		let rows = statement.query_map([], execution_program_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(execution_program_record_from_row_parts(row?)?);
		}

		drop(statement);

		for record in records {
			self.replace_program_intake_state(&record)?;
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
