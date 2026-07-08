use crate::orchestrator::tests::operator::status::running_lanes::{
	self, OffsetDateTime, RUN_ACTIVITY_MARKER_FILE, RUN_LEASE_IDLE_TIMEOUT, StateStore, fs,
	orchestrator,
};

#[test]
fn operator_status_snapshot_shadows_stale_attempt_when_newer_leased_attempt_exists() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let stale_activity =
		OffsetDateTime::now_utc().unix_timestamp() - RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 - 30;
	let stale_run_id = "pub-101-attempt-2-1781621836";
	let current_run_id = "pub-101-attempt-3-1781623863";

	state_store
		.record_run_attempt(stale_run_id, &issue.id, 2, "running")
		.expect("stale run attempt should record");
	state_store
		.record_run_attempt(current_run_id, &issue.id, 3, "running")
		.expect("current run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, current_run_id, "In Progress")
		.expect("current run lease should record");
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
			"run_id={stale_run_id}\nattempt_number=2\nlast_activity_unix_epoch={stale_activity}\nlast_protocol_activity_unix_epoch={stale_activity}\nevent_count=1\nlast_event_type=skills/changed\n"
		),
	)
	.expect("stale protocol marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].run_id, current_run_id);
	assert!(snapshot.recent_runs.iter().any(|run| run.run_id == stale_run_id));
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 1);
	assert_eq!(project.attention_count, 0);
	assert!(rendered.contains("Current lanes: 1"));
	assert!(rendered.contains("Running lanes: 1"));
	assert!(!rendered.contains(&format!("- run_id: {stale_run_id}")));
	assert!(
		rendered.contains(&format!("lifecycle_evidence: run={stale_run_id}")),
		"shadowed attempts should remain available only in lifecycle evidence"
	);
}

#[test]
fn shadows_stale_attempt_after_newer_released_lease() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let stale_activity =
		OffsetDateTime::now_utc().unix_timestamp() - RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 - 30;
	let stale_run_id = "pub-101-attempt-2-1781621836";
	let newer_run_id = "pub-101-attempt-3-1781623863";

	state_store
		.record_run_attempt(stale_run_id, &issue.id, 2, "running")
		.expect("stale run attempt should record");
	state_store
		.record_run_attempt(newer_run_id, &issue.id, 3, "succeeded")
		.expect("newer run attempt should record");
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
			"run_id={stale_run_id}\nattempt_number=2\nlast_activity_unix_epoch={stale_activity}\nlast_protocol_activity_unix_epoch={stale_activity}\nevent_count=1\nlast_event_type=skills/changed\n"
		),
	)
	.expect("stale protocol marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert!(snapshot.current_lanes.is_empty());
	assert!(snapshot.recent_runs.iter().any(|run| run.run_id == stale_run_id));
	assert!(snapshot.recent_runs.iter().any(|run| run.run_id == newer_run_id));
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 0);
}
