mod autonomy;
mod authority;
mod base;
mod control;
mod migrations;
mod programs;
mod review;

use crate::state::sqlite_store::{
	ChildAgentActivitySummary, OptionalExtension, Result, SqliteStateStore,
	execution_program_record_from_row_parts, execution_program_runtime_row_parts, eyre, params,
	timestamp_parts,
};

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
	github_owner TEXT NOT NULL,
	github_repository TEXT NOT NULL,
	tracker_team_id TEXT NOT NULL,
	routing_label TEXT NOT NULL,
	enabled INTEGER NOT NULL,
	config_fingerprint TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS projects_repository_key_idx
ON projects (github_owner, github_repository);
CREATE TABLE IF NOT EXISTS lanes (
	project_key TEXT NOT NULL,
	tracker_issue_id TEXT NOT NULL,
	binding_fingerprint TEXT NOT NULL,
	epoch INTEGER NOT NULL,
	phase TEXT NOT NULL,
	intake_authority_id TEXT,
	claim_run_id TEXT,
	admitted_base_oid TEXT,
	branch_name TEXT,
	worktree_path TEXT,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_key, tracker_issue_id)
);
DROP INDEX IF EXISTS lanes_active_tracker_issue_idx;
CREATE UNIQUE INDEX lanes_active_tracker_issue_idx
ON lanes (tracker_issue_id)
WHERE phase IN ('claimed', 'running', 'waiting_review');
CREATE TABLE IF NOT EXISTS lane_effects (
	effect_id TEXT PRIMARY KEY NOT NULL,
	operation_id TEXT NOT NULL,
	ordinal INTEGER NOT NULL,
	project_key TEXT NOT NULL,
	tracker_issue_id TEXT NOT NULL,
	journal_epoch INTEGER NOT NULL,
	kind TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	UNIQUE (operation_id, ordinal),
	FOREIGN KEY (project_key, tracker_issue_id)
		REFERENCES lanes (project_key, tracker_issue_id)
);
CREATE INDEX IF NOT EXISTS lane_effects_lane_idx
ON lane_effects (project_key, tracker_issue_id, operation_id, ordinal);
CREATE TABLE IF NOT EXISTS no_effective_delta_recoveries (
	operation_id TEXT PRIMARY KEY NOT NULL,
	project_key TEXT NOT NULL,
	tracker_issue_id TEXT NOT NULL,
	ordinal INTEGER NOT NULL CHECK (ordinal = 1),
	payload_json TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	FOREIGN KEY (project_key, tracker_issue_id)
		REFERENCES lanes (project_key, tracker_issue_id)
);
CREATE INDEX IF NOT EXISTS no_effective_delta_recoveries_lane_idx
ON no_effective_delta_recoveries (project_key, tracker_issue_id, operation_id);
CREATE TABLE IF NOT EXISTS repair_handoffs (
	handoff_id TEXT PRIMARY KEY NOT NULL,
	predecessor_project_key TEXT NOT NULL,
	predecessor_issue_id TEXT NOT NULL,
	predecessor_epoch INTEGER NOT NULL,
	successor_project_key TEXT NOT NULL,
	successor_issue_id TEXT NOT NULL,
	state TEXT NOT NULL CHECK (state IN ('active', 'replaced', 'cancelled', 'accepted', 'rejected_stale')),
	payload_json TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	FOREIGN KEY (predecessor_project_key, predecessor_issue_id)
		REFERENCES lanes (project_key, tracker_issue_id),
	FOREIGN KEY (successor_project_key, successor_issue_id)
		REFERENCES lanes (project_key, tracker_issue_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS repair_handoffs_active_idx
ON repair_handoffs (predecessor_project_key, predecessor_issue_id, predecessor_epoch)
WHERE state = 'active';
CREATE TABLE IF NOT EXISTS supersession_edges (
	edge_id TEXT PRIMARY KEY NOT NULL,
	handoff_id TEXT UNIQUE NOT NULL,
	predecessor_project_key TEXT NOT NULL,
	predecessor_issue_id TEXT NOT NULL,
	predecessor_epoch INTEGER NOT NULL,
	payload_json TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	UNIQUE (predecessor_project_key, predecessor_issue_id),
	FOREIGN KEY (handoff_id) REFERENCES repair_handoffs (handoff_id),
	FOREIGN KEY (predecessor_project_key, predecessor_issue_id)
		REFERENCES lanes (project_key, tracker_issue_id)
);
CREATE TABLE IF NOT EXISTS superseded_closeout_operations (
	operation_id TEXT PRIMARY KEY NOT NULL,
	edge_id TEXT UNIQUE NOT NULL,
	predecessor_project_key TEXT NOT NULL,
	predecessor_issue_id TEXT NOT NULL,
	stage TEXT NOT NULL,
	stage_epoch INTEGER NOT NULL,
	payload_json TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	FOREIGN KEY (predecessor_project_key, predecessor_issue_id)
		REFERENCES lanes (project_key, tracker_issue_id)
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
		#[cfg(test)]
		self.bootstrap_legacy_projection_schema()?;
		self.bootstrap_authority_event_schema()?;
		self.bootstrap_lane_schema()?;
		self.bootstrap_review_schema()?;
		self.bootstrap_evidence_artifact_schema()?;
		self.bootstrap_run_control_channels_schema()?;
		self.bootstrap_connector_backoffs_schema()?;
		self.bootstrap_private_execution_events_schema()?;
		self.bootstrap_decision_contracts_schema()?;
		self.bootstrap_autonomy_objectives_schema()?;
		self.bootstrap_autonomy_runtime_policies_schema()?;
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

	#[cfg(test)]
	fn bootstrap_legacy_projection_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS leases (
	issue_id TEXT PRIMARY KEY NOT NULL,
	project_id TEXT NOT NULL,
	run_id TEXT NOT NULL,
	issue_state TEXT NOT NULL
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
"#,
		)?;
		self.bootstrap_worktree_schema()
	}
}
