use std::fs;

use tempfile::TempDir;

use crate::state::{
	self, EffectiveRuntimeMarker, RUN_CONTROL_ACTION_COMPLETED, RUN_CONTROL_ACTION_FAILED,
	RUN_CONTROL_ACTION_FALLBACK, RUN_CONTROL_ACTION_TIMED_OUT, RunControlActionRequest, StateStore,
	tests::IN_PROGRESS_STATE,
};

#[test]
fn run_control_accepts_active_attempt_and_persists_audit() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let channel_path = temp_dir.path().join("control.channel");

	fs::write(&channel_path, "ready\n").expect("control channel should write");

	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_lease("pubfi", "issue-1", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store.record_run_attempt("run-1", "issue-1", 1, "running").expect("attempt should record");
	store.update_run_thread("run-1", "thread-1").expect("thread should record");
	store.update_run_turn("run-1", "turn-1").expect("turn should record");
	store
		.publish_run_control_channel_for_active_attempt("run-1", 1, &channel_path, "local_file")
		.expect("control channel should publish")
		.expect("active control channel should exist");

	let receipt = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			source: "test_hook",
			action: "noop",
			timeout_ms: Some(500),
			metadata: None,
			context: None,
		})
		.expect("control request should resolve");

	assert_eq!(receipt.outcome(), "accepted");
	assert_eq!(receipt.reason(), "run_lease_control_channel_resolved");
	assert!(receipt.channel().is_some());

	for (outcome, reason) in [
		(RUN_CONTROL_ACTION_COMPLETED, "noop_completed"),
		(RUN_CONTROL_ACTION_FAILED, "noop_failed"),
		(RUN_CONTROL_ACTION_TIMED_OUT, "noop_timed_out"),
		(RUN_CONTROL_ACTION_FALLBACK, "noop_fallback"),
	] {
		store
			.record_run_control_action_outcome(&receipt, outcome, reason)
			.expect("follow-up control audit should record");
	}

	drop(store);

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let events = reopened
		.list_private_execution_events("pubfi", "issue-1", "run-1", 1)
		.expect("private control audit should read");
	let outcomes = events
		.iter()
		.filter(|event| event.event_type() == "control_action")
		.filter_map(|event| event.payload().get("outcome").and_then(|value| value.as_str()))
		.collect::<Vec<_>>();

	assert_eq!(outcomes, vec!["accepted", "completed", "failed", "timed_out", "fallback"]);
}

#[test]
fn run_control_accepts_marker_hydrated_active_attempt_identity() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let channel_path = temp_dir.path().join("control.channel");
	let worktree_path = temp_dir.path().join("worktree");
	let worktree_path_text = worktree_path.to_string_lossy().to_string();

	fs::create_dir_all(&worktree_path).expect("worktree should create");
	fs::write(&channel_path, "ready\n").expect("control channel should write");

	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.upsert_lease("pubfi", "issue-1", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store
		.upsert_worktree("pubfi", "issue-1", "x/pubfi-issue-1", &worktree_path_text)
		.expect("worktree should record");
	store.record_run_attempt("run-1", "issue-1", 1, "running").expect("attempt should record");

	state::write_run_effective_runtime_marker(
		&worktree_path,
		"run-1",
		1,
		&EffectiveRuntimeMarker {
			thread_id: Some("thread-marker"),
			turn_id: Some("turn-marker"),
			effective_model: "gpt-5.4",
			effective_model_provider: "openai",
			effective_cwd: worktree_path_text.as_str(),
			effective_approval_policy: "never",
			effective_approvals_reviewer: "human",
			effective_sandbox_mode: "dangerFullAccess",
		},
	)
	.expect("runtime marker should write");

	store
		.publish_run_control_channel_for_active_attempt("run-1", 1, &channel_path, "local_file")
		.expect("control channel should publish")
		.expect("active control channel should exist");

	let receipt = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-marker"),
			turn_id: Some("turn-marker"),
			source: "test_hook",
			action: "interrupt",
			timeout_ms: Some(500),
			metadata: None,
			context: None,
		})
		.expect("marker-hydrated control request should resolve");

	assert_eq!(receipt.outcome(), "accepted");
	assert_eq!(receipt.reason(), "run_lease_control_channel_resolved");
	assert_eq!(receipt.current_thread_id(), Some("thread-marker"));
	assert_eq!(receipt.current_turn_id(), Some("turn-marker"));

	let events = store
		.list_private_execution_events("pubfi", "issue-1", "run-1", 1)
		.expect("private control audit should read");
	let event = events
		.iter()
		.find(|event| event.record_id() == receipt.audit_record_id())
		.expect("control audit event should exist");

	assert_eq!(event.payload()["observed"]["thread_id"].as_str(), Some("thread-marker"));
	assert_eq!(event.payload()["observed"]["turn_id"].as_str(), Some("turn-marker"));
}
