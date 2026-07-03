use std::{fs, process};

use tempfile::TempDir;

use crate::state::{
	self, ChildAgentActivitySummary, EffectiveRuntimeMarker, ProtocolActivityMarker,
	ProtocolActivitySummary, RUN_ACTIVITY_MARKER_FILE,
	tests::run_activity::markers::account_summary,
};

#[test]
fn run_activity_marker_round_trips_marker_surfaces() {
	assert_run_activity_marker_round_trips_clearable_auxiliary_fields();
	assert_run_activity_marker_round_trips_thread_and_protocol_summary_fields();
	assert_run_activity_marker_round_trips_child_agent_activity_summary();

	account_summary::assert_run_activity_marker_round_trips_account_summary();
	account_summary::assert_run_activity_marker_preserves_account_summary_after_activity_refresh();
	account_summary::assert_run_activity_marker_preserves_account_summary_after_stale_rewrite();
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
			fs::read_to_string(temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE))
				.expect("activity marker body should load")
				.contains(&format!("host_boot_id={host_boot_id}\n")),
			"activity markers should record the host boot identity for reboot-safe liveness"
		);
	}
	if let Some(process_start_identity) = state::current_process_start_identity() {
		assert_eq!(marker.process_start_identity(), Some(process_start_identity.as_str()));
		assert!(
			fs::read_to_string(temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE))
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
