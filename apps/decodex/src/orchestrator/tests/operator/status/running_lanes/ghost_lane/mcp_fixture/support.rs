use crate::orchestrator::tests::operator::status::running_lanes::StateStore;

pub(crate) fn append_mcp_test_fixture_control_private_events(state_store: &StateStore) {
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
}

pub(crate) fn append_mcp_test_fixture_ghost_lane_cleanup_audit(state_store: &StateStore) {
	state_store
		.append_private_execution_event(
			"pubfi",
			"PUB-012",
			"run-12",
			1,
			"ghost_lane_cleanup",
			serde_json::json!({
				"schema": "decodex.ghost_lane_recovery_private_event/1",
				"event": "ghost_lane_cleanup",
				"classification": "mcp_test_fixture_ghost_lane",
				"reason": "tracker_issue_missing_and_only_mcp_test_control_fixture_evidence",
				"issue_identifier": "PUBFI-012",
				"terminal_status": "terminal_guarded",
				"cleared_run_lease": true,
				"evidence": [
					"tracker_issue_missing",
					"worktree_mapping_path_missing",
					"worktree_missing",
					"control_channel_file_missing",
					"mcp_test_fixture_control_channel_row_present",
					"mcp_test_fixture_private_control_evidence_present",
					"review_lineage_missing"
				],
				"blockers": [],
				"next_action": "ordinary automation may continue after status readback confirms no current attention lane"
			}),
		)
		.expect("cleanup audit should record");
}
