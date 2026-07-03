mod askpass;
mod logs;
mod protocol_prune;

use std::{
	fs::{self, FileTimes, OpenOptions},
	path::Path,
	time::SystemTime,
};

use rusqlite::{Connection, OptionalExtension as _};
use tempfile::TempDir;

use crate::maintenance::{self};

const TEST_RUNTIME_SCHEMA: &str = "PRAGMA journal_mode = WAL;
		CREATE TABLE projects (
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
		CREATE TABLE leases (
			issue_id TEXT PRIMARY KEY NOT NULL,
			project_id TEXT NOT NULL,
			run_id TEXT NOT NULL,
			issue_state TEXT NOT NULL
		);
		CREATE TABLE run_attempts (
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
		CREATE TABLE protocol_events (
			run_id TEXT NOT NULL,
			sequence_number INTEGER NOT NULL,
			event_type TEXT NOT NULL,
			created_at TEXT NOT NULL,
			created_at_unix INTEGER NOT NULL,
			PRIMARY KEY (run_id, sequence_number)
		);
		CREATE TABLE worktrees (
			issue_id TEXT PRIMARY KEY NOT NULL,
			project_id TEXT NOT NULL,
			branch_name TEXT NOT NULL,
			worktree_path TEXT NOT NULL
		);
		CREATE TABLE linear_execution_events (
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
		CREATE TABLE review_lifecycle_records (
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
		);";

fn insert_attempt(connection: &Connection, run_id: &str, issue_id: &str, status: &str) {
	connection
		.execute(
			"INSERT INTO run_attempts (
					run_id, project_id, issue_id, attempt_number, status, updated_at, updated_at_unix
				) VALUES (?1, 'decodex', ?2, 1, ?3, '2026-05-01T00:00:00Z', 0)",
			rusqlite::params![run_id, issue_id, status],
		)
		.expect("attempt should insert");
}

fn insert_project(connection: &Connection, worktree_root: &Path) {
	connection
		.execute(
			"INSERT INTO projects (
					service_id, config_path, repo_root, worktree_root, workflow_path,
					tracker_api_key_env_var, github_token_env_var, enabled,
					config_fingerprint, updated_at, updated_at_unix
				) VALUES (
					'decodex', '/tmp/project.toml', '/tmp/repo', ?1, '/tmp/WORKFLOW.md',
					'LINEAR_API_KEY_HACKINK', 'GITHUB_PAT_Y', 1,
					'fingerprint', '2026-05-01T00:00:00Z', 0
				)",
			rusqlite::params![worktree_root.display().to_string()],
		)
		.expect("project should insert");
}

fn set_file_modified(path: &Path, modified: SystemTime) {
	OpenOptions::new()
		.write(true)
		.open(path)
		.expect("file should open for timestamp update")
		.set_times(FileTimes::new().set_modified(modified))
		.expect("file modified time should update");
}

fn bootstrap_test_runtime_db(temp_dir: &TempDir) -> Connection {
	let decodex_home = temp_dir.path().join(".codex/decodex");

	fs::create_dir_all(&decodex_home).expect("decodex home should create");

	let database_path = decodex_home.join("runtime.sqlite3");
	let connection = Connection::open(&database_path).expect("runtime DB should open");

	connection.execute_batch(TEST_RUNTIME_SCHEMA).expect("schema should bootstrap");

	maintenance::ensure_protocol_event_summary_table(&connection)
		.expect("summary table should create");

	connection
}

fn insert_event(connection: &Connection, run_id: &str, sequence_number: i64, created_at: i64) {
	connection
		.execute(
			"INSERT INTO protocol_events (
					run_id, sequence_number, event_type, created_at, created_at_unix
				) VALUES (?1, ?2, 'event', '2026-05-01T00:00:00Z', ?3)",
			rusqlite::params![run_id, sequence_number, created_at],
		)
		.expect("event should insert");
}

fn insert_review_lifecycle(connection: &Connection, issue_id: &str, run_id: &str, phase: &str) {
	connection
		.execute(
			"INSERT INTO review_lifecycle_records (
					project_id, issue_id, branch_name, run_id, attempt_number, pr_url,
					target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase,
					request_comment_database_id,
					request_created_at_unix_epoch, request_description_thumbs_up_count,
					request_retry_count, external_round_count, auto_merge_enabled_at_unix_epoch,
					landing_state, closeout_state, repair_attempt_count, evidence_json,
					next_action,
					updated_at, updated_at_unix
				) VALUES (
					'decodex', ?1, 'y/decodex-test', ?2, 1,
					'https://github.com/hack-ink/decodex/pull/1', 'main',
					'y/decodex-test', 'abc123', 'abc123', ?3, NULL, NULL, NULL, 0, 0, NULL,
					'not_started', 'not_started', 0, '{}', '',
					'2026-05-01T00:00:00Z', 0
				)",
			rusqlite::params![issue_id, run_id, phase],
		)
		.expect("review lifecycle should insert");
}

fn insert_linear_execution_event(
	connection: &Connection,
	issue_id: &str,
	run_id: &str,
	event_type: &str,
) {
	let idempotency_key = format!("{event_type}-{run_id}");
	let payload_json = serde_json::json!({
		"type": "decodex.linear_execution_event/1",
		"record_version": 1,
		"event_type": event_type,
		"event_timestamp": "2026-05-01T00:00:00Z",
		"idempotency_key": idempotency_key,
		"service_id": "decodex",
		"issue_id": issue_id,
		"issue_identifier": issue_id,
		"run_id": run_id,
		"attempt_number": 1
	})
	.to_string();

	connection
		.execute(
			"INSERT INTO linear_execution_events (
					idempotency_key, service_id, issue_id, event_type, event_timestamp,
					event_unix, payload_json, recorded_at, recorded_at_unix
				) VALUES (?1, 'decodex', ?2, ?3, '2026-05-01T00:00:00Z', 0, ?4,
					'2026-05-01T00:00:00Z', 0)",
			rusqlite::params![idempotency_key, issue_id, event_type, payload_json],
		)
		.expect("linear execution event should insert");
}

fn protocol_event_count(connection: &Connection, run_id: &str) -> i64 {
	connection
		.query_row(
			"SELECT COUNT(*) FROM protocol_events WHERE run_id = ?1",
			rusqlite::params![run_id],
			|row| row.get(0),
		)
		.expect("event count should read")
}

fn protocol_summary_event_count(connection: &Connection, run_id: &str) -> Option<i64> {
	connection
		.query_row(
			"SELECT event_count FROM protocol_event_summaries WHERE run_id = ?1",
			rusqlite::params![run_id],
			|row| row.get(0),
		)
		.optional()
		.expect("summary should read")
}
