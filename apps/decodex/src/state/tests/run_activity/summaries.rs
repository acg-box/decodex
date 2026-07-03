use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

use crate::state::{
	ChildAgentActivityBucket, ChildAgentActivitySummary, ProtocolActivitySummary, StateStore,
	tests::IN_PROGRESS_STATE,
};

#[test]
fn records_run_attempts_and_events() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.record_run_attempt("run-1", "PUB-101", 1, "running")
		.expect("run attempt should be recorded");
	store.update_run_thread("run-1", "thread-1").expect("thread id should be attached");
	store
		.append_event("run-1", 1, "turn/started", "{\"turn\":\"1\"}")
		.expect("event should be recorded");

	let run_attempt = store
		.run_attempt("run-1")
		.expect("run attempt query should succeed")
		.expect("run attempt should exist");

	assert_eq!(run_attempt.issue_id(), "PUB-101");
	assert_eq!(run_attempt.attempt_number(), 1);
	assert_eq!(run_attempt.status(), "running");
	assert_eq!(run_attempt.thread_id(), Some("thread-1"));
	assert_eq!(store.event_count("run-1").expect("event count should succeed"), 1);
	assert_eq!(store.next_attempt_number("PUB-101").expect("next attempt should load"), 2);
	assert_eq!(
		store.retry_budget_attempt_count("PUB-101").expect("retry budget count should load"),
		0
	);

	store.update_run_status("run-1", "interrupted").expect("status should update");

	let updated = store
		.run_attempt("run-1")
		.expect("run attempt query should succeed")
		.expect("run attempt should exist");

	assert_eq!(updated.status(), "interrupted");
	assert!(
		store
			.last_run_activity_unix_epoch("run-1")
			.expect("last activity lookup should succeed")
			.is_some()
	);
}

#[test]
fn records_run_activity_summary_for_recent_project_runs() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let child_activity = ChildAgentActivitySummary {
		buckets: vec![ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds: 12,
			event_count: 3,
			tool_call_count: 0,
			input_tokens: 1_200,
			output_tokens: 240,
			output_bytes: 0,
		}],
		current_bucket: Some(String::from("Model")),
		current_detail: Some(String::from("gpt-5")),
		current_started_unix_epoch: None,
		current_elapsed_seconds: Some(12),
		wall_seconds: 12,
		event_count: 3,
		tool_call_count: 2,
		input_tokens_current: Some(1_200),
		input_tokens_max: Some(1_200),
		input_tokens_cumulative: 1_200,
		output_tokens_cumulative: 240,
		largest_tool_output_bytes: Some(4_096),
		largest_tool_output_tool: Some(String::from("shell")),
		large_output_warnings: vec![String::from("shell output was truncated")],
	};
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		..ProtocolActivitySummary::default()
	};
	let persisted_child_activity = child_activity.clone().sealed_durable();

	{
		let store = StateStore::open(&state_path).expect("persistent state store should open");

		store
			.record_run_attempt("run-1", "PUB-101", 1, "succeeded")
			.expect("run attempt should be recorded");
		store
			.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
			.expect("project ownership should record");
		store
			.record_run_activity_summary(
				"run-1",
				1,
				Some(&child_activity),
				Some(&protocol_activity),
			)
			.expect("activity summary should persist");
	}

	let reopened = StateStore::open(&state_path).expect("persistent state store should reopen");
	let runs = reopened.list_recent_runs("pubfi", 10).expect("recent project runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-1");
	assert_eq!(runs[0].child_agent_activity(), Some(&persisted_child_activity));
	assert_eq!(runs[0].protocol_activity(), Some(&protocol_activity));
}

#[test]
fn opening_state_store_seals_durable_run_activity_summary_rows() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let child_activity = ChildAgentActivitySummary {
		buckets: vec![ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds: 12,
			event_count: 3,
			tool_call_count: 0,
			input_tokens: 1_200,
			output_tokens: 240,
			output_bytes: 0,
		}],
		current_bucket: Some(String::from("Model")),
		current_detail: Some(String::from("gpt-5")),
		current_started_unix_epoch: Some(10),
		current_elapsed_seconds: Some(8),
		wall_seconds: 12,
		event_count: 3,
		tool_call_count: 2,
		input_tokens_current: Some(1_200),
		input_tokens_max: Some(1_200),
		input_tokens_cumulative: 1_200,
		output_tokens_cumulative: 240,
		largest_tool_output_bytes: Some(4_096),
		largest_tool_output_tool: Some(String::from("shell")),
		large_output_warnings: vec![String::from("shell output was truncated")],
	};
	let unsealed_json =
		serde_json::to_string(&child_activity).expect("unsealed activity should serialize");

	StateStore::open(&state_path).expect("persistent state store should bootstrap");

	{
		let connection = Connection::open(&state_path).expect("sqlite connection should reopen");

		connection
			.execute(
				"INSERT INTO run_activity_summaries (
				 run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
				 updated_at, updated_at_unix
				 ) VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
				rusqlite::params!["run-old", 1_i64, unsealed_json, "2026-06-17T00:00:00Z", 1_i64],
			)
			.expect("unsealed activity row should insert");
	}

	StateStore::open(&state_path).expect("persistent state store should seal stored row");

	let sealed_json: String = Connection::open(&state_path)
		.expect("sqlite connection should reopen")
		.query_row(
			"SELECT child_agent_activity_json FROM run_activity_summaries WHERE run_id = ?1",
			["run-old"],
			|row| row.get(0),
		)
		.expect("sealed row should load");
	let sealed_value: Value =
		serde_json::from_str(&sealed_json).expect("sealed activity should remain json");
	let sealed_activity: ChildAgentActivitySummary =
		serde_json::from_str(&sealed_json).expect("sealed activity should deserialize");

	assert!(sealed_value["current_bucket"].is_null());
	assert!(sealed_value["current_detail"].is_null());
	assert!(sealed_value["current_started_unix_epoch"].is_null());
	assert!(sealed_value["current_elapsed_seconds"].is_null());
	assert_eq!(sealed_activity, child_activity.sealed_durable());
}

#[test]
fn lists_issue_attempts_and_protocol_event_presence() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.record_run_attempt("run-2", "PUB-101", 2, "succeeded")
		.expect("second run attempt should record");
	store
		.record_run_attempt("run-1", "PUB-101", 1, "failed")
		.expect("first run attempt should record");
	store
		.record_run_attempt("run-other", "PUB-102", 1, "succeeded")
		.expect("other issue run attempt should record");
	store.update_run_thread("run-1", "thread-1").expect("first thread should attach");
	store.update_run_thread("run-2", "thread-2").expect("second thread should attach");
	store.append_event("run-1", 1, "thread/archive", "{}").expect("archive event should record");

	let attempts =
		store.list_run_attempts_for_issue("PUB-101").expect("issue attempts should load");

	assert_eq!(attempts.len(), 2);
	assert_eq!(attempts[0].run_id(), "run-1");
	assert_eq!(attempts[0].thread_id(), Some("thread-1"));
	assert_eq!(attempts[1].run_id(), "run-2");
	assert!(store.run_has_protocol_event("run-1", "thread/archive").expect("event should load"));
	assert!(
		!store
			.run_has_protocol_event("run-2", "thread/archive")
			.expect("missing event should load")
	);
}

#[test]
fn sqlite_lists_project_attempts_and_protocol_event_presence() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
	let observer = StateStore::open(&state_path).expect("observer state store should open");

	writer
		.try_acquire_lease("decodex", "issue-1", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record project ownership");
	writer
		.record_run_attempt("run-1", "issue-1", 1, "succeeded")
		.expect("first run attempt should record");
	writer.update_run_thread("run-1", "thread-1").expect("first thread should attach");
	writer.append_event("run-1", 1, "thread/archive", "{}").expect("archive event should record");
	writer
		.try_acquire_lease("other", "issue-2", "run-2", IN_PROGRESS_STATE)
		.expect("other lease should record project ownership");
	writer
		.record_run_attempt("run-2", "issue-2", 1, "succeeded")
		.expect("other run attempt should record");

	let attempts = observer
		.list_run_attempts_for_project("decodex")
		.expect("project attempts should load from sqlite");

	assert_eq!(attempts.len(), 1);
	assert_eq!(attempts[0].run_id(), "run-1");
	assert_eq!(attempts[0].thread_id(), Some("thread-1"));
	assert!(
		observer
			.run_has_protocol_event("run-1", "thread/archive")
			.expect("sqlite event presence should load")
	);
	assert!(
		!observer
			.run_has_protocol_event("run-2", "thread/archive")
			.expect("sqlite missing event presence should load")
	);
}
