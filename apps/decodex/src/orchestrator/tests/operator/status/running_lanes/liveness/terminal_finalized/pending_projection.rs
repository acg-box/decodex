use crate::orchestrator::tests::operator::status::running_lanes::{
	self, Command, ProjectRegistration, StateStore, fs, orchestrator, state,
};

#[test]
fn operator_status_projects_terminal_finalized_run_as_pending_not_active() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&running_lanes::service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store.update_run_thread("run-1", "thread-1").expect("thread should record");
	state_store.update_run_turn("run-1", "turn-1").expect("turn should record");
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
	state_store
		.append_event("run-1", 1, "skills/changed", "{}")
		.expect("protocol event should record");
	state_store
		.append_private_execution_event(
			"pubfi",
			&issue.id,
			"run-1",
			1,
			"terminal_finalize",
			serde_json::json!({
				"path": "review_handoff",
				"mode": "handoff",
				"branch": "x/pubfi-pub-101",
				"worktree_path": worktree_path.display().to_string(),
			}),
		)
		.expect("terminal finalize event should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	let mut child = Command::new("sleep").arg("30").spawn().expect("sleep child should start");
	let child_pid = child.id();

	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, child_pid)
		.expect("live process marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	running_lanes::assert_terminal_pending_status_projection(&snapshot);
	running_lanes::assert_terminal_pending_lane_inspect(&state_store);
	running_lanes::assert_terminal_pending_interrupt_rejects_force(&state_store);

	if matches!(child.try_wait(), Ok(None)) {
		child.kill().expect("sleep child should be killable");
	}

	child.wait().expect("sleep child should reap");
}
