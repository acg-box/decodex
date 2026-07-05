use crate::orchestrator::tests::operator::status::running_lanes::{
	self, Command, Duration, Instant, StateStore, fs, orchestrator, state, thread,
};

#[test]
fn operator_status_snapshot_counts_stopped_active_process_as_attention_not_running() {
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
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, u32::MAX)
		.expect("stopped process marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.phase, "executing");
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("process_stopped"));
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 1);
}

#[cfg(unix)]
#[test]
fn operator_status_snapshot_counts_zombie_active_process_as_attention_not_running() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let mut child = Command::new("/bin/sh")
		.arg("-c")
		.arg("exit 0")
		.spawn()
		.expect("short-lived child process should spawn");
	let child_pid = child.id();

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
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, child_pid)
		.expect("zombie process marker should write");

	let deadline = Instant::now() + Duration::from_secs(5);
	let mut observed_stopped = false;

	while Instant::now() < deadline {
		if !orchestrator::process_is_alive(child_pid) {
			observed_stopped = true;

			break;
		}

		thread::sleep(Duration::from_millis(10));
	}

	let snapshot = observed_stopped.then(|| {
		orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
			.expect("snapshot should build")
	});

	child.wait().expect("short-lived child process should reap");

	assert!(observed_stopped, "exited child process must not count as alive");

	let snapshot = snapshot.expect("snapshot should be captured while child is unreaped");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.phase, "executing");
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("process_stopped"));
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 1);
}
