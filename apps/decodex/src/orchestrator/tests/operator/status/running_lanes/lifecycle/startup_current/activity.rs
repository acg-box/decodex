use crate::orchestrator::tests::operator::status::running_lanes::{
	self, EffectiveRuntimeMarker, OffsetDateTime, ProtocolActivityMarker, RUN_ACTIVITY_MARKER_FILE,
	RUN_LEASE_IDLE_TIMEOUT, StateStore, fs, orchestrator, state,
};

#[test]
fn operator_status_snapshot_promotes_starting_after_app_server_activity() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "starting")
		.expect("run attempt should record");
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

	state::write_run_thread_status_marker(
		&worktree_path,
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[],
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
			last_event_type: "model/response",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol summary should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(run.status, "running");
	assert_eq!(run.attempt_status, "starting");
	assert_eq!(run.phase, "executing");
	assert_eq!(run.queue_lease_state, "held");
	assert_eq!(run.execution_liveness, "process_alive");
	assert_eq!(run.thread_status.as_deref(), Some("active"));
	assert_eq!(run.effective_model.as_deref(), Some("gpt-5.4"));
	assert!(rendered.contains("status: running"));
	assert!(rendered.contains("attempt_status: starting"));
}

#[test]
fn operator_status_snapshot_counts_stale_starting_run_as_attention_not_running() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let stale_activity =
		OffsetDateTime::now_utc().unix_timestamp() - RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 - 30;

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "starting")
		.expect("run attempt should record");
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

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	fs::write(
		worktree_path.join(RUN_ACTIVITY_MARKER_FILE),
		format!(
			"run_id=run-1\nattempt_number=1\nlast_activity_unix_epoch={stale_activity}\nlast_protocol_activity_unix_epoch={stale_activity}\nlast_progress_unix_epoch={stale_activity}\n"
		),
	)
	.expect("stale processless marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.status, "starting");
	assert_eq!(run.phase, "executing");
	assert_eq!(run.process_alive, None);
	assert!(run.protocol_idle_for_seconds.is_some_and(|idle| {
		u64::try_from(idle).is_ok_and(|idle| idle >= RUN_LEASE_IDLE_TIMEOUT.as_secs())
	}));
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 1);
}
