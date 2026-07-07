use crate::state::sqlite_store::schema::{Result, SqliteStateStore};

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
	schema_version TEXT NOT NULL DEFAULT 'decodex/lifecycle-authority-record/1',
	subject_id TEXT NOT NULL DEFAULT '',
	sequence INTEGER NOT NULL DEFAULT 0,
	transition TEXT NOT NULL DEFAULT '',
	previous_state TEXT NOT NULL DEFAULT '',
	next_state TEXT NOT NULL DEFAULT '',
	review_level TEXT NOT NULL DEFAULT '',
	review_gate_state TEXT NOT NULL DEFAULT '',
	base_branch TEXT,
	validated_head_sha TEXT NOT NULL DEFAULT '',
	worktree_path TEXT NOT NULL DEFAULT '',
	merge_commit TEXT,
	cleanup_state TEXT NOT NULL DEFAULT 'not_started',
	authority TEXT NOT NULL DEFAULT '',
	actor TEXT NOT NULL DEFAULT '',
	source_evidence_refs_json TEXT NOT NULL DEFAULT '[]',
	idempotency_key TEXT NOT NULL DEFAULT '',
	correlation_id TEXT NOT NULL DEFAULT '',
	causation_id TEXT,
	decided_at TEXT NOT NULL DEFAULT '',
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
	pub(in crate::state) fn bootstrap_review_schema(&self) -> Result<()> {
		self.connection.execute_batch(DROP_LEGACY_REVIEW_MARKER_TABLES_SQL)?;
		self.connection.execute_batch(REVIEW_LIFECYCLE_SCHEMA_SQL)?;
		self.ensure_column(
			"review_policy_checkpoints",
			"details_json",
			"ALTER TABLE review_policy_checkpoints ADD COLUMN details_json TEXT NOT NULL DEFAULT '{}'",
		)?;
		for (column, sql) in [
			(
				"schema_version",
				"ALTER TABLE review_lifecycle_records ADD COLUMN schema_version TEXT NOT NULL DEFAULT 'decodex/lifecycle-authority-record/1'",
			),
			(
				"subject_id",
				"ALTER TABLE review_lifecycle_records ADD COLUMN subject_id TEXT NOT NULL DEFAULT ''",
			),
			(
				"sequence",
				"ALTER TABLE review_lifecycle_records ADD COLUMN sequence INTEGER NOT NULL DEFAULT 0",
			),
			(
				"transition",
				"ALTER TABLE review_lifecycle_records ADD COLUMN transition TEXT NOT NULL DEFAULT ''",
			),
			(
				"previous_state",
				"ALTER TABLE review_lifecycle_records ADD COLUMN previous_state TEXT NOT NULL DEFAULT ''",
			),
			(
				"next_state",
				"ALTER TABLE review_lifecycle_records ADD COLUMN next_state TEXT NOT NULL DEFAULT ''",
			),
			(
				"review_level",
				"ALTER TABLE review_lifecycle_records ADD COLUMN review_level TEXT NOT NULL DEFAULT ''",
			),
			(
				"review_gate_state",
				"ALTER TABLE review_lifecycle_records ADD COLUMN review_gate_state TEXT NOT NULL DEFAULT ''",
			),
			("base_branch", "ALTER TABLE review_lifecycle_records ADD COLUMN base_branch TEXT"),
			(
				"validated_head_sha",
				"ALTER TABLE review_lifecycle_records ADD COLUMN validated_head_sha TEXT NOT NULL DEFAULT ''",
			),
			(
				"worktree_path",
				"ALTER TABLE review_lifecycle_records ADD COLUMN worktree_path TEXT NOT NULL DEFAULT ''",
			),
			("merge_commit", "ALTER TABLE review_lifecycle_records ADD COLUMN merge_commit TEXT"),
			(
				"cleanup_state",
				"ALTER TABLE review_lifecycle_records ADD COLUMN cleanup_state TEXT NOT NULL DEFAULT 'not_started'",
			),
			(
				"authority",
				"ALTER TABLE review_lifecycle_records ADD COLUMN authority TEXT NOT NULL DEFAULT ''",
			),
			(
				"actor",
				"ALTER TABLE review_lifecycle_records ADD COLUMN actor TEXT NOT NULL DEFAULT ''",
			),
			(
				"source_evidence_refs_json",
				"ALTER TABLE review_lifecycle_records ADD COLUMN source_evidence_refs_json TEXT NOT NULL DEFAULT '[]'",
			),
			(
				"idempotency_key",
				"ALTER TABLE review_lifecycle_records ADD COLUMN idempotency_key TEXT NOT NULL DEFAULT ''",
			),
			(
				"correlation_id",
				"ALTER TABLE review_lifecycle_records ADD COLUMN correlation_id TEXT NOT NULL DEFAULT ''",
			),
			("causation_id", "ALTER TABLE review_lifecycle_records ADD COLUMN causation_id TEXT"),
			(
				"decided_at",
				"ALTER TABLE review_lifecycle_records ADD COLUMN decided_at TEXT NOT NULL DEFAULT ''",
			),
		] {
			self.ensure_column("review_lifecycle_records", column, sql)?;
		}

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
}
