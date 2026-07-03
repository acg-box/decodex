use crate::orchestrator::tests::operator::status::running_lanes::{
	self, FakeTracker, ReviewHandoffMarker, StateStore, TEST_SERVICE_ID, fs, orchestrator,
};

#[test]
fn live_operator_status_blocks_missing_issue_ghost_cleanup_when_review_lifecycle_exists() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let marker = ReviewHandoffMarker::new(
		"run-12",
		1,
		"x/pubfi-pub-012",
		"https://github.com/hack-ink/decodex/pull/12",
		"main",
		"x/pubfi-pub-012",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_review_handoff_marker(TEST_SERVICE_ID, "PUB-012", &marker)
		.expect("review lifecycle should record");

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
	assert!(run.lane_control_conditions.contains(&String::from("review_lifecycle_present")));
}

#[test]
fn live_operator_status_blocks_missing_issue_ghost_cleanup_when_private_evidence_exists() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.append_private_execution_event(
			"pubfi",
			"PUB-012",
			"run-12",
			1,
			"diagnostic",
			serde_json::json!({"schema": "test.private/1"}),
		)
		.expect("private evidence should record");

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
	assert!(run.lane_control_conditions.contains(&String::from("private_evidence_present")));
}

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

	append_mcp_test_fixture_control_private_events(&state_store);
	append_mcp_test_fixture_ghost_lane_cleanup_audit(&state_store);

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

	append_mcp_test_fixture_control_private_events(&state_store);
	append_mcp_test_fixture_ghost_lane_cleanup_audit(&state_store);

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

fn append_mcp_test_fixture_control_private_events(state_store: &StateStore) {
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

fn append_mcp_test_fixture_ghost_lane_cleanup_audit(state_store: &StateStore) {
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
