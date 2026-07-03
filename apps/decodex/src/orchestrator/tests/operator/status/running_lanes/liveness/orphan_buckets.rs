use crate::orchestrator::tests::operator::status::running_lanes::{
	self, StateStore, fs, orchestrator, process, state,
};

#[test]
fn operator_status_snapshot_keeps_terminal_status_live_process_in_recent_orphan_bucket() {
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

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, process::id())
		.expect("live process marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot
		.recent_runs
		.iter()
		.find(|run| run.run_id == "run-1")
		.expect("live terminal run should remain inspectable");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(run.run_id, "run-1");
	assert_eq!(run.status, "failed");
	assert_eq!(run.attempt_status, "failed");
	assert_eq!(run.status_projection_reason, None);
	assert_eq!(run.phase, "failed");
	assert!(!run.run_lease);
	assert_eq!(run.queue_lease_state, "not_held");
	assert_eq!(run.execution_liveness, "not_running");
	assert_eq!(run.ownership_state, "orphaned_live_thread");
	assert_eq!(run.liveness_state, "process_alive");
	assert_eq!(run.terminalization_state, "barrier_started");
	assert!(
		run.lane_control_conditions
			.iter()
			.any(|condition| condition == "terminal_attempt_has_live_evidence")
	);
	assert_eq!(run.process_alive, Some(true));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("process_alive"));
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.retained_worktree_count, 1);
	assert_eq!(snapshot.worktrees[0].ownership, "orphaned_live_thread");
}

#[test]
fn operator_status_snapshot_excludes_terminal_thread_archive_from_running_lanes() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "failed")
		.expect("run attempt should record");
	state_store
		.append_event("run-1", 1, "thread/archive", "{}")
		.expect("thread archive event should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert!(
		snapshot.current_lanes.is_empty(),
		"terminal archive-only protocol events must not present as active execution"
	);
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert!(
		snapshot.recent_runs.iter().all(|run| run.run_id != "run-1"),
		"archive-only terminal attempts do not need to remain operator-visible"
	);
}

#[test]
fn operator_status_snapshot_projects_terminal_run_with_active_thread_as_retained_attention() {
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

	state::write_run_thread_status_marker(
		&worktree_path,
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[],
	)
	.expect("thread status should write");
	running_lanes::rewrite_run_activity_marker_host_boot_id(&worktree_path, "previous-boot");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot
		.recent_runs
		.iter()
		.find(|run| run.run_id == "run-1")
		.expect("terminal run should remain inspectable");

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(run.status, "stalled");
	assert_eq!(run.attempt_status, "stalled");
	assert_eq!(run.status_projection_reason, None);
	assert!(!run.run_lease);
	assert_eq!(run.queue_lease_state, "not_held");
	assert_eq!(run.execution_liveness, "not_running");
	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.liveness_state, "host_boot_mismatch");
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("host_boot_id_mismatch"));
	assert_eq!(run.thread_status.as_deref(), Some("active"));
	assert!(
		run.lane_control_conditions.iter().any(|condition| condition == "host_boot_id_mismatch")
	);
}

#[test]
fn operator_status_snapshot_keeps_succeeded_status_live_process_in_recent_orphan_bucket() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "succeeded")
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
		.expect("live process marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot
		.recent_runs
		.iter()
		.find(|run| run.run_id == "run-1")
		.expect("live succeeded run should remain inspectable");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(run.run_id, "run-1");
	assert_eq!(run.status, "succeeded");
	assert_eq!(run.attempt_status, "succeeded");
	assert_eq!(run.status_projection_reason, None);
	assert_eq!(run.phase, "completed");
	assert!(!run.run_lease);
	assert_eq!(run.queue_lease_state, "not_held");
	assert_eq!(run.execution_liveness, "not_running");
	assert_eq!(run.ownership_state, "orphaned_live_thread");
	assert_eq!(run.liveness_state, "process_alive");
	assert_eq!(run.terminalization_state, "barrier_started");
	assert_eq!(run.process_alive, Some(true));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("process_alive"));
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.retained_worktree_count, 1);
	assert_eq!(snapshot.worktrees[0].ownership, "orphaned_live_thread");
}
