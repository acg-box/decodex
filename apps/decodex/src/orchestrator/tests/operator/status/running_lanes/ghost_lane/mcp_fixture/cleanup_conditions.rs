use crate::orchestrator::tests::operator::status::running_lanes::{
	self, FakeTracker, StateStore, orchestrator,
};

#[test]
fn live_operator_status_allows_mcp_test_fixture_ghost_lane_cleanup_conditions() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let missing_channel_path = config.worktree_root().join("missing-run-control.channel");

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

	for (event_type, payload) in [
		(
			"control_action",
			serde_json::json!({
				"schema": "decodex.run_control_action/v1",
				"source": "mcp-test",
				"action": "steer"
			}),
		),
		(
			"control_action",
			serde_json::json!({
				"schema": "decodex.run_control_action/v1",
				"source": "cli",
				"action": "interrupt",
				"requested": {
					"project_id": "pubfi",
					"issue_id": "PUB-012",
					"run_id": "run-12",
					"attempt_number": 1,
					"thread_id": "thread-12",
					"turn_id": "turn-12"
				}
			}),
		),
		(
			"lane_control/steer/requested",
			serde_json::json!({
				"source": "mcp-test",
				"method": "turn/steer"
			}),
		),
	] {
		state_store
			.append_private_execution_event("pubfi", "PUB-012", "run-12", 1, event_type, payload)
			.expect("mcp-test private evidence should record");
	}

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should be visible");

	assert_eq!(run.ownership_state, "ghost_lane");
	assert_eq!(run.policy_state, "runtime_recovery_required");
	assert_eq!(run.lane_control_next_action, "run_ghost_lane_recovery");
	assert!(
		run.lane_control_conditions
			.contains(&String::from("mcp_test_fixture_private_control_evidence_present"))
	);
	assert!(
		run.lane_control_conditions
			.contains(&String::from("mcp_test_fixture_protocol_or_thread_evidence_present"))
	);

	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		"PUB-012",
		"run-12",
	)
	.expect("cleanup status blockers should load");

	assert!(blockers.is_empty());
}
