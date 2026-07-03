use std::{
	fs::{self, FileTimes, OpenOptions},
	path::Path,
	time::{Duration, SystemTime},
};

use rusqlite::{Connection, OptionalExtension as _};
use tempfile::TempDir;
use time::OffsetDateTime;

use crate::{
	maintenance::{
		self, MaintenanceMode, MaintenancePolicy, MaintenancePruneRequest, MaintenanceScope,
	},
	state::StateStore,
	test_support::TestEnvVarGuard,
};

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

#[test]
fn prune_compacts_only_terminal_unowned_protocol_events() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let connection = bootstrap_test_runtime_db(&temp_dir);
	let now = OffsetDateTime::now_utc();
	let old = now.unix_timestamp() - 30 * 24 * 60 * 60;
	let fresh = now.unix_timestamp() - 2 * 24 * 60 * 60;

	insert_attempt(&connection, "old-run", "old-issue", "succeeded");
	insert_event(&connection, "old-run", 1, old);
	insert_event(&connection, "old-run", 2, old + 60);
	insert_attempt(&connection, "leased-run", "leased-issue", "running");
	insert_event(&connection, "leased-run", 1, old);
	insert_attempt(&connection, "old-leased-issue-run", "leased-issue", "succeeded");
	insert_event(&connection, "old-leased-issue-run", 1, old);

	connection
		.execute(
			"INSERT INTO leases (issue_id, project_id, run_id, issue_state)
				 VALUES ('leased-issue', 'decodex', 'leased-run', 'In Progress')",
			[],
		)
		.expect("run lease should insert");

	insert_attempt(&connection, "retained-run", "retained-issue", "failed");
	insert_event(&connection, "retained-run", 1, old);

	connection
		.execute(
			"INSERT INTO worktrees (issue_id, project_id, branch_name, worktree_path)
				 VALUES ('retained-issue', 'decodex', 'xy/retained', '/tmp/retained')",
			[],
		)
		.expect("retained worktree should insert");

	insert_attempt(&connection, "review-handoff-run", "review-issue", "succeeded");
	insert_event(&connection, "review-handoff-run", 1, old);
	insert_review_lifecycle(&connection, "review-issue", "review-handoff-run", "request_pending");
	insert_attempt(&connection, "cleanup-blocked-run", "cleanup-issue", "succeeded");
	insert_event(&connection, "cleanup-blocked-run", 1, old);
	insert_review_lifecycle(&connection, "cleanup-issue", "cleanup-blocked-run", "cleanup_blocked");
	insert_attempt(&connection, "attention-run", "attention-issue", "failed");
	insert_event(&connection, "attention-run", 1, old);
	insert_linear_execution_event(
		&connection,
		"attention-issue",
		"attention-run",
		"needs_attention",
	);
	insert_attempt(&connection, "terminal-failure-run", "failure-issue", "failed");
	insert_event(&connection, "terminal-failure-run", 1, old);
	insert_linear_execution_event(
		&connection,
		"failure-issue",
		"terminal-failure-run",
		"terminal_failure",
	);
	insert_attempt(&connection, "fresh-run", "fresh-issue", "succeeded");
	insert_event(&connection, "fresh-run", 1, fresh);

	let report = maintenance::run_prune_with_policy(
		MaintenancePruneRequest {
			mode: MaintenanceMode::Apply,
			scope: MaintenanceScope::Full,
			json: false,
		},
		MaintenancePolicy { protocol_event_retention_days: 14, ..MaintenancePolicy::default() },
	)
	.expect("maintenance should run");

	assert_eq!(report.runtime.compacted_runs, 1);
	assert_eq!(report.runtime.compacted_events, 2);
	assert_eq!(protocol_event_count(&connection, "old-run"), 0);
	assert_eq!(protocol_summary_event_count(&connection, "old-run"), Some(2));
	assert_eq!(protocol_event_count(&connection, "leased-run"), 1);
	assert_eq!(protocol_event_count(&connection, "old-leased-issue-run"), 1);
	assert_eq!(protocol_event_count(&connection, "retained-run"), 1);
	assert_eq!(protocol_event_count(&connection, "review-handoff-run"), 1);
	assert_eq!(protocol_event_count(&connection, "cleanup-blocked-run"), 1);
	assert_eq!(protocol_event_count(&connection, "attention-run"), 1);
	assert_eq!(protocol_event_count(&connection, "terminal-failure-run"), 1);
	assert_eq!(protocol_event_count(&connection, "fresh-run"), 1);
}

#[test]
fn auto_safe_prune_compacts_terminal_unowned_protocol_events() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let connection = bootstrap_test_runtime_db(&temp_dir);
	let now = OffsetDateTime::now_utc();
	let old = now.unix_timestamp() - 30 * 24 * 60 * 60;

	insert_attempt(&connection, "old-run", "old-issue", "succeeded");
	insert_event(&connection, "old-run", 1, old);

	let report = maintenance::run_prune_with_policy(
		MaintenancePruneRequest {
			mode: MaintenanceMode::Apply,
			scope: MaintenanceScope::AutoSafe,
			json: false,
		},
		MaintenancePolicy { protocol_event_retention_days: 14, ..MaintenancePolicy::default() },
	)
	.expect("auto-safe maintenance should run");

	assert_eq!(report.runtime.compacted_runs, 1);
	assert_eq!(report.runtime.compacted_events, 1);
	assert!(report.runtime.warnings.is_empty());
	assert_eq!(protocol_event_count(&connection, "old-run"), 0);
	assert_eq!(protocol_summary_event_count(&connection, "old-run"), Some(1));

	let state_store = StateStore::open(temp_dir.path().join(".codex/decodex/runtime.sqlite3"))
		.expect("state store should reopen compacted runtime DB");
	let runs = state_store
		.list_recent_runs("decodex", 10)
		.expect("recent runs should load compacted summary");
	let compacted_run = runs
		.iter()
		.find(|run| run.run_id() == "old-run")
		.expect("compacted run should remain status-visible");

	assert_eq!(compacted_run.event_count(), 1);
	assert_eq!(compacted_run.last_event_type(), Some("event"));
	assert_eq!(compacted_run.last_event_at(), Some("2026-05-01T00:00:00Z"));
}

#[test]
fn auto_safe_prune_warns_and_continues_when_runtime_candidate_detection_fails() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let decodex_home = temp_dir.path().join(".codex/decodex");

	fs::create_dir_all(&decodex_home).expect("decodex home should create");
	Connection::open(decodex_home.join("runtime.sqlite3")).expect("empty runtime DB should create");

	let report = maintenance::run_prune_with_policy(
		MaintenancePruneRequest {
			mode: MaintenanceMode::Apply,
			scope: MaintenanceScope::AutoSafe,
			json: false,
		},
		MaintenancePolicy::default(),
	)
	.expect("auto-safe maintenance should continue after candidate detection failure");

	assert_eq!(report.runtime.compacted_runs, 0);
	assert_eq!(report.runtime.warnings.len(), 1);
	assert_eq!(report.runtime.warnings[0].warning, "auto_protocol_event_compaction_skipped");
	assert_eq!(report.runtime.warnings[0].reason, "candidate_detection_failed");
}

#[test]
fn prune_rotates_oversized_logs_and_agent_evidence_events() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let log_dir = temp_dir.path().join(".codex/decodex/logs");
	let evidence_dir = temp_dir.path().join(".codex/decodex/agent-evidence/decodex");
	let log_path = log_dir.join("decodex.log");
	let events_path = evidence_dir.join("events.jsonl");

	fs::create_dir_all(&log_dir).expect("log dir should create");
	fs::create_dir_all(&evidence_dir).expect("evidence dir should create");
	fs::write(&log_path, b"0123456789abcdef").expect("log should write");
	fs::write(&events_path, b"0123456789abcdef").expect("events should write");

	let report = maintenance::run_prune_with_policy(
		MaintenancePruneRequest {
			mode: MaintenanceMode::Apply,
			scope: MaintenanceScope::AutoSafe,
			json: false,
		},
		MaintenancePolicy {
			log_rotate_bytes: 8,
			evidence_rotate_bytes: 8,
			..MaintenancePolicy::default()
		},
	)
	.expect("maintenance should run");

	assert_eq!(report.logs.rotated_files, 1);
	assert_eq!(report.agent_evidence.rotated_files, 1);
	assert_eq!(fs::metadata(&log_path).expect("log should remain").len(), 0);
	assert_eq!(fs::metadata(&events_path).expect("events should remain").len(), 0);
	assert_eq!(
		fs::read_dir(&log_dir)
			.expect("log dir should list")
			.filter_map(std::result::Result::ok)
			.filter(|entry| entry.path() != log_path)
			.count(),
		1
	);
	assert_eq!(
		fs::read_dir(&evidence_dir)
			.expect("evidence dir should list")
			.filter_map(std::result::Result::ok)
			.filter(|entry| entry.path() != events_path)
			.count(),
		1
	);
}

#[test]
fn prune_deletes_only_rotated_logs_and_agent_evidence_after_fourteen_days() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let log_dir = temp_dir.path().join(".codex/decodex/logs");
	let evidence_dir = temp_dir.path().join(".codex/decodex/agent-evidence/decodex");
	let current_log_path = log_dir.join("decodex.log");
	let old_log_path = log_dir.join("decodex.1.log");
	let fresh_log_path = log_dir.join("decodex.2.log");
	let current_events_path = evidence_dir.join("events.jsonl");
	let old_events_path = evidence_dir.join("events.1.jsonl");
	let fresh_events_path = evidence_dir.join("events.2.jsonl");
	let old_time = SystemTime::now() - Duration::from_secs(15 * 24 * 60 * 60);
	let fresh_time = SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60);

	fs::create_dir_all(&log_dir).expect("log dir should create");
	fs::create_dir_all(&evidence_dir).expect("evidence dir should create");

	for path in [
		&current_log_path,
		&old_log_path,
		&fresh_log_path,
		&current_events_path,
		&old_events_path,
		&fresh_events_path,
	] {
		fs::write(path, b"event\n").expect("maintenance fixture should write");
	}

	set_file_modified(&current_log_path, old_time);
	set_file_modified(&old_log_path, old_time);
	set_file_modified(&fresh_log_path, fresh_time);
	set_file_modified(&current_events_path, old_time);
	set_file_modified(&old_events_path, old_time);
	set_file_modified(&fresh_events_path, fresh_time);

	let report = maintenance::run_prune_with_policy(
		MaintenancePruneRequest {
			mode: MaintenanceMode::Apply,
			scope: MaintenanceScope::AutoSafe,
			json: false,
		},
		MaintenancePolicy::default(),
	)
	.expect("maintenance should run");

	assert_eq!(report.logs.deleted_files, 1);
	assert_eq!(report.agent_evidence.deleted_files, 1);
	assert!(current_log_path.exists());
	assert!(!old_log_path.exists());
	assert!(fresh_log_path.exists());
	assert!(current_events_path.exists());
	assert!(!old_events_path.exists());
	assert!(fresh_events_path.exists());
}

#[test]
fn prune_deletes_old_legacy_git_askpass_helpers_from_registered_worktree_roots() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let connection = bootstrap_test_runtime_db(&temp_dir);
	let worktree_root = temp_dir.path().join("repo/.worktrees");
	let old_helper = worktree_root.join(".decodex-git-askpass-xy-101-attempt-1.sh");
	let fresh_helper = worktree_root.join(".decodex-git-askpass-xy-102-attempt-1.sh");
	let unrelated = worktree_root.join("notes.sh");
	let old_time = SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60);
	let fresh_time = SystemTime::now();

	insert_project(&connection, &worktree_root);

	fs::create_dir_all(&worktree_root).expect("worktree root should create");
	fs::write(&old_helper, b"#!/bin/sh\n").expect("old helper should write");
	fs::write(&fresh_helper, b"#!/bin/sh\n").expect("fresh helper should write");
	fs::write(&unrelated, b"#!/bin/sh\n").expect("unrelated file should write");

	set_file_modified(&old_helper, old_time);
	set_file_modified(&fresh_helper, fresh_time);
	set_file_modified(&unrelated, old_time);

	let report = maintenance::run_prune_with_policy(
		MaintenancePruneRequest {
			mode: MaintenanceMode::Apply,
			scope: MaintenanceScope::AutoSafe,
			json: false,
		},
		MaintenancePolicy::default(),
	)
	.expect("maintenance should run");

	assert_eq!(report.git_askpass_helpers.deleted_files, 1);
	assert_eq!(report.git_askpass_helpers.delete_candidates, 1);
	assert!(!old_helper.exists());
	assert!(fresh_helper.exists());
	assert!(unrelated.exists());
}

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
