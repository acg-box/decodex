use crate::orchestrator::tests::operator::status::running_lanes::{
	self, MODEL_EXECUTION_IDLE_TIMEOUT, OffsetDateTime, ProtocolActivityMarker,
	ProtocolActivitySummary, RUN_ACTIVITY_MARKER_FILE, RUN_LEASE_IDLE_TIMEOUT,
	RUN_OPERATION_AGENT_RUN, RUN_OPERATION_RECONCILIATION, StateStore, fs, orchestrator, process,
	state,
};

#[test]
fn operator_status_snapshot_reports_retry_backoff_from_worktree_marker() {
	for (retry_kind, expected_wait_reason) in
		[("failure", "failure_retry"), ("git_lock_contention", "git_lock_contention")]
	{
		let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = running_lanes::sample_issue("Todo", &[]);
		let worktree_path = config.worktree_root().join("PUB-101");

		state_store
			.record_run_attempt("run-1", &issue.id, 1, "failed")
			.expect("run attempt should record");
		state_store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-101",
				&worktree_path.display().to_string(),
			)
			.expect("worktree should record");

		state::write_run_retry_schedule(
			&worktree_path,
			"run-1",
			1,
			retry_kind,
			OffsetDateTime::now_utc().unix_timestamp() + 60,
		)
		.expect("retry schedule marker should write");

		let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
			.expect("snapshot should build");
		let run = snapshot.recent_runs.first().expect("recent run should exist");

		assert_eq!(run.phase, "retry_backoff");
		assert_eq!(run.wait_reason.as_deref(), Some(expected_wait_reason));
		assert_eq!(run.retry_kind.as_deref(), Some(retry_kind));
		assert!(run.next_retry_at.is_some());
		assert_eq!(snapshot.projects[0].waiting_lane_count, 1);
		assert_eq!(snapshot.projects[0].connector_state, "backoff");
	}
}

#[test]
fn operator_status_snapshot_keeps_continuation_retry_from_orphaning_live_marker_worktree() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "continuation_pending")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, process::id())
		.expect("live marker should write");
	state::write_run_retry_schedule(
		&worktree_path,
		"run-1",
		1,
		"continuation",
		OffsetDateTime::now_utc().unix_timestamp() + 60,
	)
	.expect("continuation retry schedule marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.recent_runs.first().expect("recent run should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(run.status, "continuation_pending");
	assert_eq!(run.phase, "retry_backoff");
	assert_eq!(run.wait_reason.as_deref(), Some("continuation_retry"));
	assert_eq!(run.retry_kind.as_deref(), Some("continuation"));
	assert_eq!(run.ownership_state, "continuation_pending");
	assert_eq!(run.liveness_state, "process_alive");
	assert_eq!(run.lane_control_next_action, "wait_for_continuation_reentry");
	assert_eq!(snapshot.worktrees[0].ownership, "continuation_pending");
	assert_eq!(snapshot.projects[0].retained_worktree_count, 0);
	assert_eq!(snapshot.projects[0].waiting_lane_count, 1);
	assert!(rendered.contains("Recovery worktrees: 0"));
	assert!(rendered.contains("- none (owned worktrees are shown in their lane sections above)"));
	assert!(!rendered.contains("role: orphaned_live_thread"));
}

#[test]
fn operator_status_snapshot_ignores_retry_schedule_on_running_attempt() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, process::id())
		.expect("run marker should write");
	state::write_run_retry_schedule(
		&worktree_path,
		"run-1",
		1,
		"failure",
		OffsetDateTime::now_utc().unix_timestamp() + 60,
	)
	.expect("stale retry schedule marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");

	assert_eq!(run.phase, "executing");
	assert_eq!(run.wait_reason, None);
	assert_eq!(run.retry_kind, None);
	assert_eq!(run.next_retry_at, None);
	assert_eq!(snapshot.projects[0].waiting_lane_count, 0);
	assert_eq!(snapshot.projects[0].connector_state, "ok");
}

#[test]
fn operator_status_snapshot_reports_stalled_runs_explicitly() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "stalled")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.recent_runs.first().expect("recent run should exist");

	assert_eq!(run.phase, "stalled");
	assert_eq!(run.wait_reason.as_deref(), Some("app_server_idle_timeout"));
	assert_eq!(run.current_operation, state::RUN_OPERATION_IDLE);
	assert!(!run.suspected_stall);
}

#[test]
fn operator_status_snapshot_surfaces_reconciliation_operation_for_stalled_runs() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "stalled")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_operation_marker(&worktree_path, "run-1", 1, RUN_OPERATION_RECONCILIATION)
		.expect("reconciliation marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.recent_runs.first().expect("recent run should exist");

	assert_eq!(run.phase, "stalled");
	assert_eq!(run.current_operation, state::RUN_OPERATION_RECONCILIATION);
}

#[test]
fn operator_status_snapshot_preserves_stalled_run_activity_when_tagging_reconciliation() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "stalled")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, 42)
		.expect("initial activity marker should write");

	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = fs::read_to_string(&marker_path).expect("marker body should load");
	let stale_activity = OffsetDateTime::now_utc().unix_timestamp() - 600;
	let rewritten = marker_body
		.lines()
		.map(|line| {
			if line.starts_with("last_activity_unix_epoch=") {
				format!("last_activity_unix_epoch={stale_activity}")
			} else if line.starts_with("last_progress_unix_epoch=") {
				format!("last_progress_unix_epoch={stale_activity}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
		+ "\n";

	fs::write(&marker_path, rewritten).expect("marker body should rewrite");
	state::write_run_operation_marker_preserving_activity(
		&worktree_path,
		"run-1",
		1,
		RUN_OPERATION_RECONCILIATION,
	)
	.expect("reconciliation marker should preserve existing activity");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.recent_runs.first().expect("recent run should exist");

	assert_eq!(marker.process_id(), Some(42));
	assert_eq!(marker.last_activity_unix_epoch(), Some(stale_activity));
	assert_eq!(run.current_operation, state::RUN_OPERATION_RECONCILIATION);
	assert_eq!(run.process_id, Some(42));
}

#[test]
fn operator_status_snapshot_marks_soft_stalls_before_hard_timeout() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_operation_marker(&worktree_path, "run-1", 1, RUN_OPERATION_AGENT_RUN)
		.expect("operation marker should write");

	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = fs::read_to_string(&marker_path).expect("marker body should load");
	let suspected_age = (RUN_LEASE_IDLE_TIMEOUT.as_secs() / 2).saturating_add(1) as i64;
	let stale_progress = OffsetDateTime::now_utc().unix_timestamp() - suspected_age;
	let rewritten = marker_body
		.lines()
		.map(|line| {
			if line.starts_with("last_progress_unix_epoch=") {
				format!("last_progress_unix_epoch={stale_progress}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
		+ "\n";

	fs::write(&marker_path, rewritten).expect("marker body should rewrite");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");

	assert_eq!(run.current_operation, state::RUN_OPERATION_AGENT_RUN);
	assert!(run.last_progress_at.is_some());
	assert!(run.suspected_stall);
}

#[test]
fn operator_status_snapshot_diagnoses_protocol_only_model_execution() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_operation_marker(&worktree_path, "run-1", 1, RUN_OPERATION_AGENT_RUN)
		.expect("operation marker should write");

	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = fs::read_to_string(&marker_path).expect("marker body should load");
	let suspected_age = (MODEL_EXECUTION_IDLE_TIMEOUT.as_secs() / 2).saturating_add(1) as i64;
	let stale_progress = OffsetDateTime::now_utc().unix_timestamp() - suspected_age;
	let rewritten = marker_body
		.lines()
		.map(|line| {
			if line.starts_with("last_progress_unix_epoch=") {
				format!("last_progress_unix_epoch={stale_progress}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
		+ "\n";

	fs::write(&marker_path, rewritten).expect("marker body should rewrite");

	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model_execution")),
		recent_events: vec![
			state::ProtocolActivityEventSummary {
				event_type: String::from("thread/status/changed"),
				category: String::from("thread"),
				detail: Some(String::from("active")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("thread/goal/updated"),
				category: String::from("protocol"),
				detail: Some(String::from("active")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("account/rateLimits/updated"),
				category: String::from("rate_limit"),
				detail: Some(String::from("pro")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("account/rateLimits/updated"),
				category: String::from("rate_limit"),
				detail: Some(String::from("pro")),
			},
		],
		..ProtocolActivitySummary::default()
	};

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 2,
			last_event_type: "account/rateLimits/updated",
			child_agent_activity: None,
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol-only marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(run.wait_reason.as_deref(), Some("model_execution"));
	assert_eq!(run.progress_diagnostic.as_deref(), Some("protocol_only_activity"));
	assert_eq!(run.execution_liveness, "process_alive");
	assert!(run.suspected_stall);
	assert_ne!(run.last_progress_at, run.last_protocol_activity_at);
	assert!(rendered.contains("progress_diagnostic: protocol_only_activity"));
}
