use crate::orchestrator::tests::operator::status::running_lanes::{
	self, FakeTracker, StateStore, fs, orchestrator,
};

#[test]
fn live_operator_status_allows_ghost_recovery_when_worktree_mapping_path_is_missing() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let missing_worktree_path = config.worktree_root().join("PUB-012");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			"PUB-012",
			"x/pubfi-pub-012",
			&missing_worktree_path.display().to_string(),
		)
		.expect("stale worktree mapping should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("ghost current lane should be visible");

	assert_eq!(run.ownership_state, "ghost_lane");
	assert_eq!(run.policy_state, "runtime_recovery_required");
	assert_eq!(run.lane_control_next_action, "run_ghost_lane_recovery");
	assert!(run.lane_control_conditions.contains(&String::from("worktree_mapping_path_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("worktree_missing")));
	assert!(!run.lane_control_conditions.contains(&String::from("retained_worktree_present")));
}

#[test]
fn live_operator_status_blocks_missing_issue_ghost_cleanup_when_retained_worktree_exists() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());

	fs::create_dir_all(config.worktree_root().join("PUB-012"))
		.expect("retained worktree directory should exist");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should be visible");

	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.policy_state, "runtime_recovery_blocked");
	assert_eq!(run.lane_control_next_action, "inspect_missing_issue_runtime_recovery_blockers");
	assert!(run.needs_attention);
	assert!(!run.counts_as_running);
	assert!(run.lane_control_conditions.contains(&String::from("retained_worktree_present")));
}

#[test]
fn live_operator_status_blocks_missing_issue_ghost_cleanup_when_control_channel_row_exists() {
	let (temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let channel_path = temp_dir.path().join("missing-control-channel.json");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.publish_run_control_channel_for_active_attempt("run-12", 1, &channel_path, "local_file")
		.expect("control channel row should publish");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should be visible");

	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.policy_state, "runtime_recovery_blocked");
	assert_eq!(run.lane_control_next_action, "inspect_missing_issue_runtime_recovery_blockers");
	assert!(run.needs_attention);
	assert!(!run.counts_as_running);
	assert!(run.lane_control_conditions.contains(&String::from("control_channel_file_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("control_channel_present")));
}
