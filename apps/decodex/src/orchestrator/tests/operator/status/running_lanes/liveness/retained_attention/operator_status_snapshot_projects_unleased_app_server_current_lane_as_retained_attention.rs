use crate::orchestrator::tests::operator::status::running_lanes::{
	self, StateStore, orchestrator, state,
};

#[test]
fn operator_status_snapshot_projects_unleased_app_server_current_lane_as_retained_attention() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
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
	running_lanes::rewrite_run_activity_marker_host_boot_id(&worktree_path, "previous-boot");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(run.run_id, "run-1");
	assert_eq!(run.status, "running");
	assert_eq!(run.phase, "executing");
	assert!(!run.run_lease);
	assert_eq!(run.queue_lease_state, "not_held");
	assert_eq!(run.execution_liveness, "process_identity_mismatch");
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("host_boot_id_mismatch"));
	assert_eq!(run.thread_status.as_deref(), Some("active"));
	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.liveness_state, "host_boot_mismatch");
	assert_eq!(run.lane_control_next_action, "inspect_recovery_evidence");
	assert!(
		run.lane_control_conditions.iter().any(|condition| condition == "host_boot_id_mismatch")
	);
	assert!(run.has_fresh_execution);
	assert!(!run.counts_as_running);
	assert!(run.needs_attention);
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 1);
	assert_eq!(snapshot.worktrees[0].ownership, "retained_attention");
}
