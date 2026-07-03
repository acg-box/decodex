use std::{
	fs::{File, ReadDir},
	io::{self, Read as _, Write as _},
	path::Path,
	process, slice,
};

use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;
use time::OffsetDateTime;

use crate::state::{
	self, ChildAgentActivityBucket, ChildAgentActivitySummary, CodexAccountActivitySummary,
	CodexAccountMarker, EffectiveRuntimeMarker, ProtocolActivityMarker, ProtocolActivitySummary,
	RUN_ACTIVITY_MARKER_FILE, RUN_OPERATION_REPO_GATE, ReviewHandoffMarker,
	ReviewOrchestrationMarker, ReviewPolicyCheckpointInput, StateStore,
	tests::{self, IN_PROGRESS_STATE},
};

struct MarkerFile;
impl MarkerFile {
	fn read_dir(path: impl AsRef<Path>) -> io::Result<ReadDir> {
		path.as_ref().read_dir()
	}

	fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
		let mut file = File::open(path)?;
		let mut body = String::new();

		file.read_to_string(&mut body)?;

		Ok(body)
	}

	fn write(path: impl AsRef<Path>, body: impl AsRef<[u8]>) -> io::Result<()> {
		let mut file = File::create(path)?;

		file.write_all(body.as_ref())
	}
}

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

#[test]
fn run_activity_marker_round_trips_marker_surfaces() {
	assert_run_activity_marker_round_trips_clearable_auxiliary_fields();
	assert_run_activity_marker_round_trips_thread_and_protocol_summary_fields();
	assert_run_activity_marker_round_trips_child_agent_activity_summary();
	assert_run_activity_marker_round_trips_account_summary();
	assert_run_activity_marker_preserves_account_summary_after_activity_refresh();
	assert_run_activity_marker_preserves_account_summary_after_stale_rewrite();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_host_boot_id_uses_boot_session_uuid() {
	let host_boot_id = state::current_host_boot_id().expect("macOS boot session UUID should read");

	assert!(
		host_boot_id.starts_with("macos_bootsessionuuid:"),
		"macOS host boot identity should use boot-session UUID, got {host_boot_id}"
	);
	assert!(
		!host_boot_id.contains("boottime") && !host_boot_id.contains("usec"),
		"macOS host boot identity should not depend on kern.boottime timeval output"
	);
}

fn assert_run_activity_marker_round_trips_clearable_auxiliary_fields() {
	let temp_dir = TempDir::new().expect("tempdir should create");

	state::write_run_activity_marker_for_process(temp_dir.path(), "run-1", 1, process::id())
		.expect("activity marker should write");
	state::write_run_retry_schedule(temp_dir.path(), "run-1", 1, "failure", 12_345)
		.expect("retry schedule should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.run_id(), "run-1");
	assert_eq!(marker.attempt_number(), 1);

	if let Some(host_boot_id) = state::current_host_boot_id() {
		assert_eq!(marker.host_boot_id(), Some(host_boot_id.as_str()));
		assert!(
			MarkerFile::read_to_string(temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE))
				.expect("activity marker body should load")
				.contains(&format!("host_boot_id={host_boot_id}\n")),
			"activity markers should record the host boot identity for reboot-safe liveness"
		);
	}
	if let Some(process_start_identity) = state::current_process_start_identity() {
		assert_eq!(marker.process_start_identity(), Some(process_start_identity.as_str()));
		assert!(
			MarkerFile::read_to_string(temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE))
				.expect("activity marker body should load")
				.contains(&format!("process_start_identity={process_start_identity}\n")),
			"activity markers should record the process start identity for PID-reuse-safe liveness"
		);
	}

	assert_eq!(marker.retry_kind(), Some("failure"));
	assert_eq!(marker.retry_ready_at_unix_epoch(), Some(12_345));

	state::clear_run_retry_schedule(temp_dir.path()).expect("retry schedule should clear");

	let retry_cleared = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should reload")
		.expect("marker snapshot should still exist");

	assert_eq!(retry_cleared.retry_kind(), None);
	assert_eq!(retry_cleared.retry_ready_at_unix_epoch(), None);
}

fn assert_run_activity_marker_round_trips_thread_and_protocol_summary_fields() {
	let temp_dir = TempDir::new().expect("tempdir should create");

	state::write_run_activity_marker_for_process(temp_dir.path(), "run-1", 1, process::id())
		.expect("activity marker should write");
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
		&[String::from("waitingOnApproval")],
	)
	.expect("thread status marker should write");
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
			effective_sandbox_mode: "workspaceWrite",
		},
	)
	.expect("effective runtime marker should write");

	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		waiting_reason: Some(String::from("model_execution")),
		rate_limit_status: Some(String::from("usageLimitExceeded")),
		recent_events: vec![
			state::ProtocolActivityEventSummary {
				event_type: String::from("turn/started"),
				category: String::from("turn"),
				detail: Some(String::from("running")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("turn/completed"),
				category: String::from("turn"),
				detail: Some(String::from("completed")),
			},
		],
	};

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
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol summary should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.thread_id(), Some("thread-1"));
	assert_eq!(marker.turn_id(), Some("turn-1"));
	assert_eq!(marker.thread_status(), Some("active"));
	assert_eq!(marker.thread_active_flags(), &[String::from("waitingOnApproval")]);
	assert_eq!(marker.event_count(), 3);
	assert_eq!(marker.last_event_type(), Some("turn/completed"));
	assert_eq!(marker.effective_model(), Some("gpt-5.4"));
	assert_eq!(marker.effective_model_provider(), Some("openai"));
	assert_eq!(marker.effective_cwd(), Some("/tmp/worktree"));
	assert_eq!(marker.effective_approval_policy(), Some("never"));
	assert_eq!(marker.effective_approvals_reviewer(), Some("human"));
	assert_eq!(marker.effective_sandbox_mode(), Some("workspaceWrite"));
	assert_eq!(marker.protocol_activity(), Some(&protocol_activity));
	assert!(marker.last_protocol_activity_unix_epoch().is_some());
	assert_eq!(marker.current_operation(), Some(state::RUN_OPERATION_AGENT_RUN));
	assert!(marker.last_progress_unix_epoch().is_some());
}

fn assert_run_activity_marker_round_trips_child_agent_activity_summary() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let summary = ChildAgentActivitySummary {
		buckets: vec![
			state::ChildAgentActivityBucket {
				name: String::from("Model"),
				wall_seconds: 693,
				event_count: 12,
				tool_call_count: 0,
				input_tokens: 4_270_000,
				output_tokens: 12_000,
				output_bytes: 0,
			},
			state::ChildAgentActivityBucket {
				name: String::from("Browser/Image"),
				wall_seconds: 41,
				event_count: 6,
				tool_call_count: 3,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 180_000,
			},
		],
		current_bucket: Some(String::from("Model")),
		current_detail: Some(String::from("waiting after tool output")),
		current_started_unix_epoch: Some(1_800_000_000),
		current_elapsed_seconds: Some(9),
		wall_seconds: 734,
		event_count: 18,
		tool_call_count: 3,
		input_tokens_current: Some(105_000),
		input_tokens_max: Some(105_000),
		input_tokens_cumulative: 4_270_000,
		output_tokens_cumulative: 12_000,
		largest_tool_output_bytes: Some(180_000),
		largest_tool_output_tool: Some(String::from("view_image")),
		large_output_warnings: vec![String::from(
			"view_image repeated 3 large outputs; largest 180000 bytes",
		)],
	};

	state::write_run_protocol_activity_marker(
		temp_dir.path(),
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 18,
			last_event_type: "item/tool/call/response",
			child_agent_activity: Some(&summary),
			protocol_activity: None,
		},
	)
	.expect("protocol summary should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.child_agent_activity(), Some(&summary));
}

#[test]
fn run_protocol_non_work_events_do_not_refresh_progress_marker() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let stale_progress = OffsetDateTime::now_utc().unix_timestamp() - 3_600;

	MarkerFile::write(
		temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE),
		format!(
			"run_id=run-1\nattempt_number=1\nlast_activity_unix_epoch={stale_progress}\nlast_protocol_activity_unix_epoch={stale_progress}\nlast_progress_unix_epoch={stale_progress}\n"
		),
	)
	.expect("initial marker should write");

	let account_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model_execution")),
		recent_events: vec![state::ProtocolActivityEventSummary {
			event_type: String::from("account/rateLimits/updated"),
			category: String::from("rate_limit"),
			detail: Some(String::from("pro")),
		}],
		..ProtocolActivitySummary::default()
	};

	write_test_protocol_activity_marker(
		temp_dir.path(),
		1,
		"account/rateLimits/updated",
		Some(&account_activity),
	)
	.expect("account protocol activity should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.last_progress_unix_epoch(), Some(stale_progress));
	assert!(
		marker
			.last_protocol_activity_unix_epoch()
			.is_some_and(|last_protocol| last_protocol > stale_progress)
	);

	let first_protocol_activity = marker
		.last_protocol_activity_unix_epoch()
		.expect("account protocol activity should update protocol time");

	write_test_protocol_activity_marker(
		temp_dir.path(),
		2,
		"account/rateLimits/updated",
		Some(&account_activity),
	)
	.expect("second account protocol activity should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.last_progress_unix_epoch(), Some(stale_progress));
	assert!(
		marker
			.last_protocol_activity_unix_epoch()
			.is_some_and(|last_protocol| last_protocol >= first_protocol_activity)
	);

	let goal_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model_execution")),
		recent_events: vec![state::ProtocolActivityEventSummary {
			event_type: String::from("thread/goal/updated"),
			category: String::from("protocol"),
			detail: Some(String::from("active")),
		}],
		..ProtocolActivitySummary::default()
	};

	write_test_protocol_activity_marker(
		temp_dir.path(),
		3,
		"thread/goal/updated",
		Some(&goal_activity),
	)
	.expect("goal status protocol activity should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.last_progress_unix_epoch(), Some(stale_progress));

	write_test_protocol_activity_marker(temp_dir.path(), 4, "item/fileChange/patchUpdated", None)
		.expect("work protocol activity should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert!(
		marker
			.last_progress_unix_epoch()
			.is_some_and(|last_progress| last_progress > stale_progress)
	);
}

fn write_test_protocol_activity_marker(
	worktree_path: &Path,
	event_count: i64,
	last_event_type: &str,
	protocol_activity: Option<&ProtocolActivitySummary>,
) -> state::Result<()> {
	state::write_run_protocol_activity_marker(
		worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count,
			last_event_type,
			child_agent_activity: None,
			protocol_activity,
		},
	)
}

fn assert_run_activity_marker_round_trips_account_summary() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let summary = sample_codex_account_activity_summary();

	state::write_run_account_marker(
		temp_dir.path(),
		&CodexAccountMarker {
			run_id: "run-1",
			attempt_number: 1,
			account: &summary,
			accounts: slice::from_ref(&summary),
		},
	)
	.expect("account summary should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.account(), Some(&summary));
	assert_eq!(marker.accounts(), slice::from_ref(&summary));

	let body = MarkerFile::read_to_string(temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE))
		.expect("marker body should read");

	assert!(body.contains("account="));
	assert!(body.contains("accounts="));
	assert!(!body.contains("codex_account="));
	assert!(!body.contains("codex_accounts="));
}

fn assert_run_activity_marker_preserves_account_summary_after_activity_refresh() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let summary = sample_codex_account_activity_summary();

	state::write_run_account_marker(
		temp_dir.path(),
		&CodexAccountMarker {
			run_id: "run-1",
			attempt_number: 1,
			account: &summary,
			accounts: slice::from_ref(&summary),
		},
	)
	.expect("account summary should write");
	state::write_run_activity_marker_at(
		temp_dir.path(),
		"run-1",
		1,
		process::id(),
		1_800_000_020,
		Some(1_800_000_019),
	)
	.expect("activity refresh should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.account(), Some(&summary));
	assert_eq!(marker.accounts(), slice::from_ref(&summary));

	let leftover_temp_marker = MarkerFile::read_dir(temp_dir.path())
		.expect("tempdir should be readable")
		.filter_map(|entry| entry.ok())
		.any(|entry| entry.file_name().to_string_lossy().contains(".decodex-run-activity."));

	assert!(!leftover_temp_marker, "atomic marker rewrites should not leave temp files");
}

fn assert_run_activity_marker_preserves_account_summary_after_stale_rewrite() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let summary = sample_codex_account_activity_summary();

	state::write_run_activity_marker_for_process(temp_dir.path(), "run-1", 1, process::id())
		.expect("initial activity marker should write");

	let stale_activity_marker = state::read_run_activity_marker_record(temp_dir.path())
		.expect("activity marker should read")
		.expect("activity marker should exist");

	state::write_run_account_marker(
		temp_dir.path(),
		&CodexAccountMarker {
			run_id: "run-1",
			attempt_number: 1,
			account: &summary,
			accounts: slice::from_ref(&summary),
		},
	)
	.expect("account summary should write");
	state::write_run_activity_marker_record(temp_dir.path(), &stale_activity_marker)
		.expect("stale activity marker rewrite should preserve current account");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.account(), Some(&summary));
	assert_eq!(marker.accounts(), slice::from_ref(&summary));
}

fn sample_codex_account_activity_summary() -> CodexAccountActivitySummary {
	CodexAccountActivitySummary {
		account_fingerprint: String::from("acct_...cdef"),
		email: Some(String::from("account@example.com")),
		plan_type: Some(String::from("pro")),
		status: String::from("selected"),
		refresh_status: String::from("not_needed"),
		checked_at_unix_epoch: Some(1_800_000_010),
		selected_at_unix_epoch: Some(1_800_000_011),
		primary_window_seconds: Some(18_000),
		primary_remaining_percent: Some(72),
		primary_resets_at_unix_epoch: Some(1_800_018_000),
		secondary_window_seconds: Some(604_800),
		secondary_remaining_percent: Some(91),
		secondary_resets_at_unix_epoch: Some(1_800_604_800),
		credits_has_credits: Some(true),
		credits_unlimited: Some(false),
		credits_balance: Some(String::from("9.99")),
		rate_limit_reached_type: None,
		cooldown_until_unix_epoch: None,
		note: Some(String::from("usage probe ok")),
		..CodexAccountActivitySummary::default()
	}
}

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
