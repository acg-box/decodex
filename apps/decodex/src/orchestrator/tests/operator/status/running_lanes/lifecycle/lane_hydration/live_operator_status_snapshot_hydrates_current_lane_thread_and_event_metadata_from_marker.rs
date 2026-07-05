use crate::orchestrator::tests::operator::status::running_lanes::{
	self, EffectiveRuntimeMarker, FakeTracker, ProtocolActivityMarker, StateStore, TEST_SERVICE_ID,
	fs, orchestrator, state, tracker,
};

#[test]
fn live_operator_status_snapshot_hydrates_current_lane_thread_and_event_metadata_from_marker() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = running_lanes::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker(&worktree_path, "run-1", 1)
		.expect("activity marker should write");
	state::write_run_thread_marker(&worktree_path, "run-1", 1, "thread-1")
		.expect("thread marker should write");
	state::write_run_turn_marker(&worktree_path, "run-1", 1, "turn-1")
		.expect("turn marker should write");
	state::write_run_thread_status_marker(
		&worktree_path,
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[String::from("waitingOnApproval")],
	)
	.expect("thread status should write");
	state::write_run_effective_runtime_marker(
		&worktree_path,
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
	.expect("effective runtime should write");
	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 2,
			last_event_type: "turn/completed",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol summary should write");

	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should succeed");

	orchestrator::hydrate_status_snapshot_state(&config, &state_store, recovered_state)
		.expect("status hydration should succeed");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].thread_id.as_deref(), Some("thread-1"));
	assert_eq!(snapshot.current_lanes[0].turn_id.as_deref(), Some("turn-1"));
	assert_eq!(snapshot.current_lanes[0].thread_status.as_deref(), Some("active"));
	assert_eq!(
		snapshot.current_lanes[0].thread_active_flags,
		vec![String::from("waitingOnApproval")]
	);
	assert!(snapshot.current_lanes[0].interactive_requested);
	assert_eq!(snapshot.current_lanes[0].event_count, 2);
	assert_eq!(snapshot.current_lanes[0].last_event_type.as_deref(), Some("turn/completed"));
	assert_eq!(snapshot.current_lanes[0].effective_model.as_deref(), Some("gpt-5.4"));
	assert_eq!(snapshot.current_lanes[0].effective_model_provider.as_deref(), Some("openai"));
	assert_eq!(snapshot.current_lanes[0].effective_approval_policy.as_deref(), Some("never"));
	assert_eq!(snapshot.current_lanes[0].effective_sandbox_mode.as_deref(), Some("workspaceWrite"));
	assert!(snapshot.current_lanes[0].last_event_at.is_some());
}
