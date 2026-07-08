use crate::orchestrator::tests::operator::status::running_lanes::{
	self, StateStore, orchestrator, state,
};

#[test]
fn no_shadowing_post_review_lane() {
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

	let mut snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	snapshot.post_review_lanes = vec![orchestrator::OperatorPostReviewLaneStatus {
		project_id: String::from("pubfi"),
		issue_id: issue.id.clone(),
		issue_identifier: issue.identifier.clone(),
		issue_state: String::from("In Review"),
		branch_name: String::from("x/pubfi-pub-101"),
		worktree_path: String::from(".worktrees/PUB-101"),
		classification: String::from("blocked"),
		reason: String::from("review_handoff_lineage_mismatch"),
		pr_url: Some(String::from("https://github.com/hack-ink/pubfi-mono-v2/pull/101")),
		pr_head_sha: Some(String::from("1111111111111111111111111111111111111111")),
		pr_state: Some(String::from("OPEN")),
		review_decision: Some(String::from("CHANGES_REQUESTED")),
		mergeable: Some(String::from("UNKNOWN")),
		check_state: Some(String::from("SUCCESS")),
		unresolved_review_threads: Some(1),
		shadowed_by_current_lane: false,
		readback_warning: None,
		readback_root_cause: Some(String::from("lineage_validation_failed")),
		loop_status: None,
	}];

	orchestrator::hydrate_post_review_lane_current_lane_shadowing(&mut snapshot);
	orchestrator::refresh_operator_project_summary(&mut snapshot, None);

	let project = snapshot.projects.first().expect("project summary should exist");
	let lane = snapshot.post_review_lanes.first().expect("post-review lane should remain visible");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(snapshot.current_lanes[0].has_fresh_execution);
	assert!(!snapshot.current_lanes[0].counts_as_running);
	assert!(snapshot.current_lanes[0].needs_attention);
	assert_eq!(snapshot.current_lanes[0].ownership_state, "retained_attention");
	assert!(!lane.shadowed_by_current_lane);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.post_review_lane_count, 1);
	assert_eq!(project.waiting_lane_count, 0);
	assert_eq!(project.attention_count, 1);
	assert!(rendered.contains("shadowed_by_current_lane: no"));
	assert!(rendered.contains("readback_root_cause: lineage_validation_failed"));
}
