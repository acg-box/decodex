use std::path::Path;

use crate::{
	recovery::{
		GHOST_LANE_CLEANUP_EVENT, GHOST_LANE_TERMINAL_STATUS,
		MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION,
	},
	state::{ProtocolActivitySummary, StateStore},
};

pub(in crate::recovery::tests) fn seed_mcp_test_fixture_ghost_lane(
	store: &StateStore,
	worktree_root: &Path,
) {
	let channel_path = worktree_root.join("missing-run-control.channel");
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		waiting_reason: Some(String::from("turn_completed")),
		rate_limit_status: None,
		recent_events: Vec::new(),
	};

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");
	store.update_run_thread("run-12", "thread-12").expect("thread should record");
	store.update_run_turn("run-12", "turn-12").expect("turn should record");
	store
		.publish_run_control_channel_for_active_attempt("run-12", 1, &channel_path, "local_file")
		.expect("control channel row should publish");
	store
		.append_event("run-12", 1, "turn/completed", r#"{"status":"completed"}"#)
		.expect("protocol event should record");
	store
		.record_run_activity_summary("run-12", 1, None, Some(&protocol_activity))
		.expect("protocol activity should record");

	append_mcp_test_control_private_events(store);
}

pub(in crate::recovery::tests) fn append_mcp_test_fixture_ghost_lane_cleanup_audit(
	store: &StateStore,
) {
	store
		.append_private_execution_event(
			"pubfi",
			"PUB-012",
			"run-12",
			1,
			GHOST_LANE_CLEANUP_EVENT,
			serde_json::json!({
				"schema": "decodex.ghost_lane_recovery_private_event/1",
				"event": GHOST_LANE_CLEANUP_EVENT,
				"classification": MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION,
				"reason": "tracker_issue_missing_and_only_mcp_test_control_fixture_evidence",
				"issue_identifier": "PUBFI-012",
				"terminal_status": GHOST_LANE_TERMINAL_STATUS,
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

fn append_mcp_test_control_private_events(store: &StateStore) {
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
		(
			"lane_control/interrupt/requested",
			serde_json::json!({
				"source": "mcp-test",
				"method": "turn/interrupt"
			}),
		),
	] {
		store
			.append_private_execution_event("pubfi", "PUB-012", "run-12", 1, event_type, payload)
			.expect("mcp-test private evidence should record");
	}
}
