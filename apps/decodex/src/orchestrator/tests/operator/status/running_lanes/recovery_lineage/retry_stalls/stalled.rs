use crate::orchestrator::tests::operator::status::running_lanes::{
	self, OffsetDateTime, RUN_ACTIVITY_MARKER_FILE, RUN_LEASE_IDLE_TIMEOUT,
	RUN_OPERATION_AGENT_RUN, RUN_OPERATION_RECONCILIATION, StateStore, fs, orchestrator, state,
};

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
fn preserves_stalled_activity_when_tagging() {
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
