use std::{
	fs::{File, ReadDir},
	io::{self, Read as _, Write as _},
	path::Path,
	process, slice,
};

use tempfile::TempDir;
use time::OffsetDateTime;

use crate::state::{
	self, ChildAgentActivitySummary, CodexAccountActivitySummary, CodexAccountMarker,
	EffectiveRuntimeMarker, ProtocolActivityMarker, ProtocolActivitySummary,
	RUN_ACTIVITY_MARKER_FILE,
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
