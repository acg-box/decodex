use std::{path::Path, process};

use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::{
	self, EffectiveRuntimeMarker, ProtocolActivityMarker, RUN_OPERATION_REPO_GATE,
	ReviewHandoffMarker, ReviewOrchestrationMarker, ReviewPolicyCheckpointInput, StateStore, tests,
};

#[test]
fn run_operation_marker_resets_stale_per_attempt_fields_on_new_attempt() {
	let temp_dir = TempDir::new().expect("tempdir should create");

	state::write_run_activity_marker_for_process(temp_dir.path(), "run-1", 1, process::id())
		.expect("first activity marker should write");
	state::write_run_thread_marker(temp_dir.path(), "run-1", 1, "thread-1")
		.expect("thread marker should write");
	state::write_run_turn_marker(temp_dir.path(), "run-1", 1, "turn-1")
		.expect("turn marker should write");
	state::write_run_thread_status_marker(
		temp_dir.path(),
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[String::from("waitingOnUserInput")],
	)
	.expect("thread status should write");
	state::write_run_effective_runtime_marker(
		temp_dir.path(),
		"run-1",
		1,
		&EffectiveRuntimeMarker {
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			effective_model: "gpt-5.4",
			effective_model_provider: "openai",
			effective_cwd: "/tmp/worktree",
			effective_approval_policy: "never",
			effective_approvals_reviewer: "human",
			effective_sandbox_mode: "dangerFullAccess",
		},
	)
	.expect("effective runtime should write");
	state::write_run_protocol_activity_marker(
		temp_dir.path(),
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 3,
			last_event_type: "turn/completed",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol summary should write");
	state::write_run_retry_schedule(temp_dir.path(), "run-1", 1, "failure", 123)
		.expect("retry schedule should write");
	state::write_run_retry_budget_attempt_count(temp_dir.path(), "run-1", 1, 2)
		.expect("retry budget should write");
	state::write_run_operation_marker(temp_dir.path(), "run-2", 2, RUN_OPERATION_REPO_GATE)
		.expect("next attempt operation marker should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.run_id(), "run-2");
	assert_eq!(marker.attempt_number(), 2);
	assert_eq!(marker.current_operation(), Some(state::RUN_OPERATION_REPO_GATE));
	assert!(marker.last_progress_unix_epoch().is_some());
	assert_eq!(marker.thread_id(), None);
	assert_eq!(marker.turn_id(), None);
	assert_eq!(marker.thread_status(), None);
	assert!(marker.thread_active_flags().is_empty());
	assert_eq!(marker.event_count(), 0);
	assert_eq!(marker.last_event_type(), None);
	assert_eq!(marker.protocol_activity(), None);
	assert_eq!(marker.effective_model(), None);
	assert_eq!(marker.effective_model_provider(), None);
	assert_eq!(marker.effective_cwd(), None);
	assert_eq!(marker.effective_approval_policy(), None);
	assert_eq!(marker.effective_approvals_reviewer(), None);
	assert_eq!(marker.effective_sandbox_mode(), None);
	assert_eq!(marker.last_protocol_activity_unix_epoch(), None);
	assert_eq!(marker.retry_kind(), None);
	assert_eq!(marker.retry_ready_at_unix_epoch(), None);
	assert_eq!(
		state::read_run_retry_budget_attempt_count(temp_dir.path())
			.expect("retry budget count should load"),
		Some(2)
	);
}

#[test]
fn counts_retry_budget_attempts_per_issue() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store.record_run_attempt("run-1", "PUB-101", 1, "succeeded").expect("first run should record");
	store.record_run_attempt("run-2", "PUB-101", 2, "failed").expect("second run should record");
	store
		.record_run_attempt("run-3", "PUB-101", 3, "interrupted")
		.expect("third run should record");
	store
		.record_run_attempt("run-5", "PUB-101", 4, "terminal_guarded")
		.expect("guarded run should record");
	store
		.record_run_attempt("run-4", "PUB-102", 1, "failed")
		.expect("other issue run should record");

	assert_eq!(
		store.retry_budget_attempt_count("PUB-101").expect("retry budget count should load"),
		3
	);
	assert_eq!(
		store.retry_budget_attempt_count("PUB-102").expect("retry budget count should load"),
		1
	);
}

#[test]
fn loads_latest_run_attempt_for_issue() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store.record_run_attempt("run-1", "PUB-101", 1, "failed").expect("first run should record");
	store
		.record_run_attempt("run-2", "PUB-101", 2, "terminal_guarded")
		.expect("latest run should record");

	let attempt = store
		.latest_run_attempt_for_issue("PUB-101")
		.expect("latest run lookup should succeed")
		.expect("latest run should exist");

	assert_eq!(attempt.run_id(), "run-2");
	assert_eq!(attempt.attempt_number(), 2);
	assert_eq!(attempt.status(), "terminal_guarded");
}

#[test]
fn manages_worktree_mappings() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_worktree("pubfi", "PUB-101", "x/pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree mapping should be recorded");

	let mapping = store
		.worktree_for_issue("PUB-101")
		.expect("mapping lookup should succeed")
		.expect("mapping should exist");

	assert_eq!(mapping.issue_id(), "PUB-101");
	assert_eq!(mapping.branch_name(), "x/pub-101");
	assert_eq!(mapping.worktree_path(), Path::new("/tmp/worktrees/pub-101"));
	assert_eq!(mapping.project_id(), "pubfi");
	assert_eq!(mapping.provenance().source(), "runtime_recorded");
	assert!(mapping.provenance().created_at_unix().is_some());
	assert!(mapping.provenance().updated_at_unix().is_some());
	assert_eq!(store.list_worktrees("pubfi").expect("list should succeed").len(), 1);

	store.clear_worktree("PUB-101").expect("mapping should be deleted");

	assert!(store.worktree_for_issue("PUB-101").expect("lookup should succeed").is_none());
}

#[test]
fn opens_legacy_worktree_rows_with_unknown_provenance() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let db_path = temp_dir.path().join("runtime.sqlite3");

	{
		let connection = Connection::open(&db_path).expect("legacy db should open");

		connection
			.execute_batch(
				"CREATE TABLE worktrees (
					issue_id TEXT PRIMARY KEY NOT NULL,
					project_id TEXT NOT NULL,
					branch_name TEXT NOT NULL,
					worktree_path TEXT NOT NULL
				);
				INSERT INTO worktrees (issue_id, project_id, branch_name, worktree_path)
				VALUES ('issue-legacy', 'pubfi', 'x/pubfi-pub-101', '/tmp/worktrees/pub-101');",
			)
			.expect("legacy worktree row should write");
	}

	let store = StateStore::open(&db_path).expect("state store should migrate");
	let mapping = store
		.worktree_for_issue("issue-legacy")
		.expect("mapping lookup should succeed")
		.expect("legacy mapping should exist");

	assert_eq!(mapping.provenance().source(), "legacy_unknown");
	assert_eq!(mapping.provenance().created_at_unix(), None);
	assert_eq!(mapping.provenance().updated_at_unix(), None);
}

#[test]
fn persistent_clear_worktree_deletes_review_lifecycle() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let handoff = ReviewHandoffMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let orchestration = ReviewOrchestrationMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);

	store
		.upsert_worktree("pubfi", "PUB-101", "x/decodex-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree mapping should be recorded");
	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("handoff projection should persist");
	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &orchestration)
		.expect("orchestration projection should persist");
	store.clear_worktree("PUB-101").expect("worktree cleanup should persist");

	let reopened = StateStore::open(&state_path).expect("reopened store should open");

	assert!(
		reopened.worktree_for_issue("PUB-101").expect("worktree lookup should succeed").is_none()
	);
	assert!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("handoff lookup should succeed")
			.is_none()
	);
	assert!(
		reopened
			.review_orchestration_marker("pubfi", "PUB-101", &handoff)
			.expect("orchestration lookup should succeed")
			.is_none()
	);
}

#[test]
fn persistent_clear_worktree_mapping_preserves_review_lifecycle() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let handoff = tests::sample_pub_101_review_handoff();

	store
		.upsert_worktree("pubfi", "PUB-101", "x/decodex-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree mapping should be recorded");
	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("handoff projection should persist");
	store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			run_id: "run-1",
			attempt_number: 1,
			phase: "handoff",
			review_level: "normal",
			status: "clean",
			head_sha: "08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review checkpoint should persist");
	store.clear_worktree_mapping("PUB-101").expect("worktree mapping cleanup should persist");

	let reopened = StateStore::open(&state_path).expect("reopened store should open");

	assert!(
		reopened.worktree_for_issue("PUB-101").expect("worktree lookup should succeed").is_none()
	);
	assert!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("handoff lookup should succeed")
			.is_some()
	);
	assert!(
		reopened
			.review_policy_checkpoint("pubfi", "PUB-101", "run-1", 1, "handoff")
			.expect("review checkpoint lookup should succeed")
			.is_some()
	);
}
