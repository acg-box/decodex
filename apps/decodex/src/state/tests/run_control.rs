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

#[test]
fn run_control_rejects_stale_turn_and_run_mismatch() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let channel_path = temp_dir.path().join("control.channel");

	fs::write(&channel_path, "ready\n").expect("control channel should write");

	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.upsert_lease("pubfi", "issue-1", "run-current", IN_PROGRESS_STATE)
		.expect("lease should record");
	store
		.record_run_attempt("run-current", "issue-1", 1, "running")
		.expect("attempt should record");
	store.update_run_thread("run-current", "thread-1").expect("thread should record");
	store.update_run_turn("run-current", "turn-current").expect("turn should record");
	store
		.publish_run_control_channel_for_active_attempt(
			"run-current",
			1,
			&channel_path,
			"local_file",
		)
		.expect("control channel should publish");

	let stale_turn = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-current",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-old"),
			source: "test_hook",
			action: "steer",
			timeout_ms: None,
			metadata: None,
			context: None,
		})
		.expect("stale turn should be audited");
	let stale_run = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-old",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-current"),
			source: "test_hook",
			action: "noop",
			timeout_ms: None,
			metadata: None,
			context: None,
		})
		.expect("stale run should be audited");

	assert_eq!(stale_turn.outcome(), "rejected");
	assert_eq!(stale_turn.reason(), "turn_mismatch");
	assert_eq!(stale_turn.current_turn_id(), Some("turn-current"));
	assert_eq!(stale_run.outcome(), "rejected");
	assert_eq!(stale_run.reason(), "run_not_found");

	let events = store
		.list_private_execution_events("pubfi", "issue-1", "run-current", 1)
		.expect("private control audit should read");
	let stale_turn_event = events
		.iter()
		.find(|event| event.record_id() == stale_turn.audit_record_id())
		.expect("stale turn audit event should exist");

	assert_eq!(
		stale_turn_event.payload()["failure_class"].as_str(),
		Some("stale_expected_turn_id")
	);
	assert_eq!(stale_turn_event.payload()["observed"]["turn_id"].as_str(), Some("turn-current"));
}

#[test]
fn run_control_rejects_missing_channel_file() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let channel_path = temp_dir.path().join("control.channel");

	fs::write(&channel_path, "ready\n").expect("control channel should write");

	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.upsert_lease("pubfi", "issue-1", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store.record_run_attempt("run-1", "issue-1", 1, "running").expect("attempt should record");
	store
		.publish_run_control_channel_for_active_attempt("run-1", 1, &channel_path, "local_file")
		.expect("control channel should publish");

	fs::remove_file(&channel_path).expect("control channel should be removable");

	let receipt = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-1",
			attempt_number: 1,
			thread_id: None,
			turn_id: None,
			source: "test_hook",
			action: "noop",
			timeout_ms: None,
			metadata: None,
			context: None,
		})
		.expect("missing channel should be audited");

	assert_eq!(receipt.outcome(), "rejected");
	assert_eq!(receipt.reason(), "control_channel_missing");
}

#[test]
fn run_control_requires_run_lease_ownership() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let channel_path = temp_dir.path().join("control.channel");
	let worktree_path = temp_dir.path().join("PUB-101");

	fs::write(&channel_path, "ready\n").expect("control channel should write");

	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.upsert_lease("pubfi", "issue-1", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store.record_run_attempt("run-1", "issue-1", 1, "running").expect("attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			"issue-1",
			"x/pubfi-issue-1",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.publish_run_control_channel_for_active_attempt("run-1", 1, &channel_path, "local_file")
		.expect("control channel should publish");
	store.clear_lease("issue-1").expect("lease should clear");

	let no_lease = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-1",
			attempt_number: 1,
			thread_id: None,
			turn_id: None,
			source: "test_hook",
			action: "noop",
			timeout_ms: None,
			metadata: None,
			context: None,
		})
		.expect("missing lease should be audited");

	store
		.upsert_lease("pubfi", "issue-1", "run-other", IN_PROGRESS_STATE)
		.expect("other lease should record");

	let wrong_run = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-1",
			attempt_number: 1,
			thread_id: None,
			turn_id: None,
			source: "test_hook",
			action: "noop",
			timeout_ms: None,
			metadata: None,
			context: None,
		})
		.expect("wrong run lease should be audited");

	assert_eq!(no_lease.outcome(), "rejected");
	assert_eq!(no_lease.reason(), "run_lease_missing");
	assert_eq!(wrong_run.outcome(), "rejected");
	assert_eq!(wrong_run.reason(), "run_lease_mismatch");

	let events = store
		.list_private_execution_events("pubfi", "issue-1", "run-1", 1)
		.expect("private control audit should read");
	let no_lease_event = events
		.iter()
		.find(|event| event.record_id() == no_lease.audit_record_id())
		.expect("missing lease audit event should exist");
	let expected_worktree_path = worktree_path.display().to_string();
	let expected_channel_path = channel_path.display().to_string();

	assert_eq!(no_lease_event.payload()["lane"]["run_lease"].as_bool(), Some(false));
	assert_eq!(no_lease_event.payload()["lane"]["attempt_status"].as_str(), Some("running"));
	assert_eq!(no_lease_event.payload()["lane"]["branch"].as_str(), Some("x/pubfi-issue-1"));
	assert_eq!(
		no_lease_event.payload()["lane"]["worktree_path"].as_str(),
		Some(expected_worktree_path.as_str())
	);
	assert_eq!(no_lease_event.payload()["channel"]["status"].as_str(), Some("active"));
	assert_eq!(no_lease_event.payload()["channel"]["path_exists"].as_bool(), Some(true));
	assert_eq!(
		no_lease_event.payload()["channel"]["channel_path"].as_str(),
		Some(expected_channel_path.as_str())
	);
}
