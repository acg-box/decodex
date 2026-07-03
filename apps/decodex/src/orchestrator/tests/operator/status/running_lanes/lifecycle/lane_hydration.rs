use crate::orchestrator::tests::operator::status::running_lanes::{
	self, EffectiveRuntimeMarker, FakeTracker, ProtocolActivityMarker, RecoveredRuntimeState,
	StateStore, TEST_SERVICE_ID, fs, orchestrator, state, tracker,
};
#[test]
fn operator_status_snapshot_keeps_all_current_lanes_when_recent_runs_are_limited() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let first_issue = running_lanes::sample_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let second_issue = running_lanes::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:17:17.133Z",
	);

	for (run_id, issue, branch_suffix) in
		[("run-1", &first_issue, "101"), ("run-2", &second_issue, "102")]
	{
		state_store
			.record_run_attempt(run_id, &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease("pubfi", &issue.id, run_id, "In Progress")
			.expect("lease should record");
		state_store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				&format!("x/pubfi-pub-{branch_suffix}"),
				&config.worktree_root().join(&issue.identifier).display().to_string(),
			)
			.expect("worktree should record");
	}

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 1)
		.expect("snapshot should build");

	assert_eq!(snapshot.run_limit, 1);
	assert_eq!(snapshot.recent_runs.len(), 2);
	assert_eq!(snapshot.current_lanes.len(), 2);
	assert!(snapshot.current_lanes.iter().all(|run| run.run_lease));
}

#[test]
fn operator_status_snapshot_keeps_terminal_run_after_lane_cleanup() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"Done",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);

	state_store.record_run_attempt("run-done", &issue.id, 1, "running").expect("run should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-done", "In Progress")
		.expect("run lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should record");
	state_store.update_run_status("run-done", "succeeded").expect("terminal status should update");
	state_store.clear_lease(&issue.id).expect("terminal cleanup should clear run lease");
	state_store.clear_worktree(&issue.id).expect("terminal cleanup should clear worktree");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 25)
		.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(snapshot.recent_runs.len(), 1);
	assert_eq!(snapshot.recent_runs[0].run_id, "run-done");
	assert_eq!(snapshot.recent_runs[0].phase, "completed");
	assert!(!snapshot.recent_runs[0].run_lease);
	assert_eq!(snapshot.recent_runs[0].branch_name, None);
	assert_eq!(snapshot.recent_runs[0].worktree_path, None);
	assert_eq!(snapshot.history_lanes.len(), 1);
	assert_eq!(snapshot.history_lanes[0].latest_run.run_id, "run-done");
	assert!(rendered.contains("Run ledger shown: 1 issue lanes from 1 history attempts"));
	assert!(rendered.contains("run_id: run-done"));
}

#[test]
fn status_hydration_does_not_fabricate_run_leases_for_recovered_candidates() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	orchestrator::hydrate_status_snapshot_state(
		&config,
		&state_store,
		RecoveredRuntimeState { recoverable_issues: vec![issue.clone()] },
	)
	.expect("status hydration should succeed");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	assert!(
		snapshot.current_lanes.is_empty(),
		"recovered retry candidates should not appear as run leased runs"
	);
	assert!(
		snapshot.recent_runs.is_empty(),
		"status hydration should not persist synthetic recovered runs"
	);
}

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
