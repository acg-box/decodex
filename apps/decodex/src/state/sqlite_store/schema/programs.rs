use crate::state::sqlite_store::schema::{
	self, Result, SqliteStateStore, execution_program_runtime_row_parts,
};

impl SqliteStateStore {
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
CREATE TABLE IF NOT EXISTS program_intake_attempts (
	project_id TEXT NOT NULL,
	contract_id TEXT NOT NULL,
	canonical_key TEXT NOT NULL,
	request_digest TEXT NOT NULL,
	status TEXT NOT NULL CHECK(status IN ('prepared', 'started', 'completed')),
	created_at TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	PRIMARY KEY (project_id, contract_id),
	UNIQUE (project_id, canonical_key)
);
CREATE INDEX IF NOT EXISTS program_intake_attempts_contract_idx
ON program_intake_attempts (project_id, contract_id, status);
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
			records.push(schema::execution_program_record_from_row_parts(row?)?);
		}

		drop(statement);

		for record in records {
			self.replace_program_intake_state(&record)?;
		}

		Ok(())
	}
}
