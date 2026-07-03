use std::fs::{self};

use rusqlite::Connection;
use tempfile::TempDir;
use time::OffsetDateTime;

use crate::{
	maintenance::{
		self, MaintenanceMode, MaintenancePolicy, MaintenancePruneRequest, MaintenanceScope, tests,
	},
	state::StateStore,
	test_support::TestEnvVarGuard,
};

fn insert_protocol_prune_fixture(connection: &Connection, old: i64, fresh: i64) {
	tests::insert_attempt(connection, "old-run", "old-issue", "succeeded");
	tests::insert_event(connection, "old-run", 1, old);
	tests::insert_event(connection, "old-run", 2, old + 60);
	tests::insert_attempt(connection, "leased-run", "leased-issue", "running");
	tests::insert_event(connection, "leased-run", 1, old);
	tests::insert_attempt(connection, "old-leased-issue-run", "leased-issue", "succeeded");
	tests::insert_event(connection, "old-leased-issue-run", 1, old);

	connection
		.execute(
			"INSERT INTO leases (issue_id, project_id, run_id, issue_state)
				 VALUES ('leased-issue', 'decodex', 'leased-run', 'In Progress')",
			[],
		)
		.expect("run lease should insert");

	tests::insert_attempt(connection, "retained-run", "retained-issue", "failed");
	tests::insert_event(connection, "retained-run", 1, old);

	connection
		.execute(
			"INSERT INTO worktrees (issue_id, project_id, branch_name, worktree_path)
				 VALUES ('retained-issue', 'decodex', 'xy/retained', '/tmp/retained')",
			[],
		)
		.expect("retained worktree should insert");

	tests::insert_attempt(connection, "review-handoff-run", "review-issue", "succeeded");
	tests::insert_event(connection, "review-handoff-run", 1, old);
	tests::insert_review_lifecycle(
		connection,
		"review-issue",
		"review-handoff-run",
		"request_pending",
	);
	tests::insert_attempt(connection, "cleanup-blocked-run", "cleanup-issue", "succeeded");
	tests::insert_event(connection, "cleanup-blocked-run", 1, old);
	tests::insert_review_lifecycle(
		connection,
		"cleanup-issue",
		"cleanup-blocked-run",
		"cleanup_blocked",
	);
	tests::insert_attempt(connection, "attention-run", "attention-issue", "failed");
	tests::insert_event(connection, "attention-run", 1, old);
	tests::insert_linear_execution_event(
		connection,
		"attention-issue",
		"attention-run",
		"needs_attention",
	);
	tests::insert_attempt(connection, "terminal-failure-run", "failure-issue", "failed");
	tests::insert_event(connection, "terminal-failure-run", 1, old);
	tests::insert_linear_execution_event(
		connection,
		"failure-issue",
		"terminal-failure-run",
		"terminal_failure",
	);
	tests::insert_attempt(connection, "fresh-run", "fresh-issue", "succeeded");
	tests::insert_event(connection, "fresh-run", 1, fresh);
}

fn assert_protocol_prune_fixture(connection: &Connection) {
	assert_eq!(tests::protocol_event_count(connection, "old-run"), 0);
	assert_eq!(tests::protocol_summary_event_count(connection, "old-run"), Some(2));
	assert_eq!(tests::protocol_event_count(connection, "leased-run"), 1);
	assert_eq!(tests::protocol_event_count(connection, "old-leased-issue-run"), 1);
	assert_eq!(tests::protocol_event_count(connection, "retained-run"), 1);
	assert_eq!(tests::protocol_event_count(connection, "review-handoff-run"), 1);
	assert_eq!(tests::protocol_event_count(connection, "cleanup-blocked-run"), 1);
	assert_eq!(tests::protocol_event_count(connection, "attention-run"), 1);
	assert_eq!(tests::protocol_event_count(connection, "terminal-failure-run"), 1);
	assert_eq!(tests::protocol_event_count(connection, "fresh-run"), 1);
}

#[test]
fn prune_compacts_only_terminal_unowned_protocol_events() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let connection = tests::bootstrap_test_runtime_db(&temp_dir);
	let now = OffsetDateTime::now_utc();
	let old = now.unix_timestamp() - 30 * 24 * 60 * 60;
	let fresh = now.unix_timestamp() - 2 * 24 * 60 * 60;

	insert_protocol_prune_fixture(&connection, old, fresh);

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

	assert_protocol_prune_fixture(&connection);
}

#[test]
fn auto_safe_prune_compacts_terminal_unowned_protocol_events() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let connection = tests::bootstrap_test_runtime_db(&temp_dir);
	let now = OffsetDateTime::now_utc();
	let old = now.unix_timestamp() - 30 * 24 * 60 * 60;

	tests::insert_attempt(&connection, "old-run", "old-issue", "succeeded");
	tests::insert_event(&connection, "old-run", 1, old);

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
	assert_eq!(tests::protocol_event_count(&connection, "old-run"), 0);
	assert_eq!(tests::protocol_summary_event_count(&connection, "old-run"), Some(1));

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
