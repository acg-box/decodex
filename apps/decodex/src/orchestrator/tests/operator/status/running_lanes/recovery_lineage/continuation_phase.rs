use crate::orchestrator::tests::operator::status::running_lanes::{
	self, PHASE_GOAL_RECOVERY_EVENT_TYPE, StateStore, TEST_SERVICE_ID,
	VALIDATION_EVIDENCE_EVENT_TYPE, orchestrator, tracker,
};

#[test]
fn operator_status_snapshot_surfaces_repeated_continuation_recovery_lineage() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let worktree_path = config.worktree_root().join("PUB-101");

	for (run_id, attempt_number) in [("run-1", 1), ("run-2", 2)] {
		state_store
			.append_private_execution_event(
				TEST_SERVICE_ID,
				&issue.id,
				run_id,
				attempt_number,
				PHASE_GOAL_RECOVERY_EVENT_TYPE,
				serde_json::json!({
					"schema": "decodex.phase_goal_signal/1",
					"phase": "implement_to_validation_ready",
					"signal": "phase_goal_recovered",
					"payload": {
						"nextPhase": "repair_validation_failures",
						"sourceErrorClass": "app_server_preflight_timeout",
						"sourceErrorMessage": "Timed out while waiting for app-server output.",
					},
				}),
			)
			.expect("phase goal recovery event should record");
	}

	state_store
		.record_run_attempt("run-3", &issue.id, 3, "running")
		.expect("current run attempt should record");
	state_store
		.upsert_lease(TEST_SERVICE_ID, &issue.id, "run-3", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let recovery = run
		.continuation_recovery
		.as_ref()
		.expect("continuation recovery lineage should project onto current lane");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(recovery.state, "continuation_scheduled");
	assert_eq!(recovery.source_phase, "implement_to_validation_ready");
	assert_eq!(recovery.next_phase, "repair_validation_failures");
	assert_eq!(recovery.source_error_class, "app_server_preflight_timeout");
	assert_eq!(recovery.recovery_count, 2);
	assert_eq!(recovery.automatic_continuation_limit, 1);
	assert!(recovery.budget_exceeded);
	assert_eq!(run.policy_state, "continuation_recovery_churn_exceeded");
	assert!(
		run.lane_control_conditions
			.contains(&String::from("continuation_recovery_budget_exceeded"))
	);
	assert!(rendered.contains("continuation_recovery: state=continuation_scheduled"));
	assert!(rendered.contains("count=2/1 budget_exceeded=yes"));
	assert_eq!(snapshot_json["current_lanes"][0]["continuation_recovery"]["budget_exceeded"], true);
}

#[test]
fn operator_status_snapshot_surfaces_validation_evidence() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"run-1",
			1,
			VALIDATION_EVIDENCE_EVENT_TYPE,
			serde_json::json!({
				"schema": "decodex.validation_evidence/2",
				"record_version": 2,
				"phase": "implement_to_validation_ready",
				"decision": "fail",
				"reason_code": "no_effective_delta",
				"objective_coverage": { "covered": true },
				"effective_delta": {
					"present": false,
					"changed_surfaces": ["runtime"],
				},
				"non_goal_check": {
					"passed": true,
					"blocker_count": 0,
				},
				"validation_evidence": {
					"repo_gate_passed": true,
				},
				"next_action": "produce an issue-scoped effective delta before completing the phase goal again",
			}),
		)
		.expect("validation evidence should record");
	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("current run attempt should record");
	state_store
		.upsert_lease(TEST_SERVICE_ID, &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let acceptance = run
		.validation_evidence
		.as_ref()
		.expect("validation evidence should project onto current lane");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(acceptance.decision, "fail");
	assert_eq!(acceptance.reason_code, "no_effective_delta");
	assert_eq!(acceptance.changed_surfaces, vec![String::from("runtime")]);
	assert!(rendered.contains("validation_evidence: phase=implement_to_validation_ready"));
	assert!(rendered.contains("reason=no_effective_delta"));
}
