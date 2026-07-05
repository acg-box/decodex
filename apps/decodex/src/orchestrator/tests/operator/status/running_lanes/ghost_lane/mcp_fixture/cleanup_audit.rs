use crate::orchestrator::tests::operator::status::running_lanes::{
	self, FakeTracker, StateStore, fs, ghost_lane::mcp_fixture::support, orchestrator,
};

#[test]
fn live_operator_status_drops_cleanup_audited_mcp_test_fixture_ghost_lane() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let missing_channel_path = config.worktree_root().join("missing-run-control.channel");
	let missing_worktree_path = config.worktree_root().join("PUB-012");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store.update_run_thread("run-12", "thread-12").expect("thread should record");
	state_store.update_run_turn("run-12", "turn-12").expect("turn should record");
	state_store
		.publish_run_control_channel_for_active_attempt(
			"run-12",
			1,
			&missing_channel_path,
			"local_file",
		)
		.expect("control channel row should publish");
	state_store
		.upsert_worktree(
			"pubfi",
			"PUB-012",
			"x/pubfi-012",
			&missing_worktree_path.display().to_string(),
		)
		.expect("stale worktree mapping should record");

	support::append_mcp_test_fixture_control_private_events(&state_store);
	support::append_mcp_test_fixture_ghost_lane_cleanup_audit(&state_store);

	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		"PUB-012",
		"run-12",
	)
	.expect("prior cleanup audit should be accepted as safe recovery evidence");

	assert!(blockers.is_empty());

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert!(
		snapshot.current_lanes.is_empty(),
		"cleanup-audited fixture ghost lane must not remain current"
	);
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 0);
	assert_eq!(
		snapshot.recent_runs[0].ownership_state, "closed",
		"cleanup-audited fixture must not leave a projected leased recent run"
	);
	assert_eq!(
		snapshot.worktrees[0].ownership, "cleanup_only",
		"cleanup-audited fixture must not leave a current-lane worktree owner"
	);
}

#[test]
fn live_operator_status_keeps_cleanup_audited_mcp_fixture_blocked_when_worktree_exists() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let missing_channel_path = config.worktree_root().join("missing-run-control.channel");

	fs::create_dir_all(config.worktree_root().join("PUB-012"))
		.expect("retained worktree directory should exist");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store.update_run_thread("run-12", "thread-12").expect("thread should record");
	state_store.update_run_turn("run-12", "turn-12").expect("turn should record");
	state_store
		.publish_run_control_channel_for_active_attempt(
			"run-12",
			1,
			&missing_channel_path,
			"local_file",
		)
		.expect("control channel row should publish");

	support::append_mcp_test_fixture_control_private_events(&state_store);
	support::append_mcp_test_fixture_ghost_lane_cleanup_audit(&state_store);

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
	assert!(run.lane_control_conditions.contains(&String::from("retained_worktree_present")));
}
