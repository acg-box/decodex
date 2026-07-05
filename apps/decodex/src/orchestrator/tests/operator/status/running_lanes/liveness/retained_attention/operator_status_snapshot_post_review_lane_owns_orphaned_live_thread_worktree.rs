use crate::orchestrator::tests::operator::status::running_lanes::{
	self, StateStore, fs, orchestrator, process, state,
};

#[test]
fn operator_status_snapshot_post_review_lane_owns_orphaned_live_thread_worktree() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("In Review", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "succeeded")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, process::id())
		.expect("live process marker should write");

	let mut snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	snapshot.post_review_lanes = vec![orchestrator::OperatorPostReviewLaneStatus {
		project_id: String::from("pubfi"),
		issue_id: issue.id.clone(),
		issue_identifier: issue.identifier.clone(),
		issue_state: String::from("In Review"),
		branch_name: String::from("x/pubfi-pub-101"),
		worktree_path: String::from(".worktrees/PUB-101"),
		classification: String::from("wait_for_review"),
		reason: String::from("non_github_review_waiting_gates"),
		pr_url: Some(String::from("https://github.com/hack-ink/pubfi-mono-v2/pull/101")),
		pr_head_sha: Some(String::from("1111111111111111111111111111111111111111")),
		pr_state: Some(String::from("OPEN")),
		review_decision: None,
		mergeable: Some(String::from("MERGEABLE")),
		check_state: Some(String::from("PENDING")),
		unresolved_review_threads: Some(0),
		shadowed_by_current_lane: false,
		readback_warning: None,
		readback_root_cause: None,
		loop_status: None,
	}];

	orchestrator::hydrate_post_review_lane_current_lane_shadowing(&mut snapshot);
	orchestrator::refresh_worktree_ownership(&mut snapshot, None);
	orchestrator::refresh_operator_project_summary(&mut snapshot, None);

	let project = snapshot.projects.first().expect("project summary should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(snapshot.recent_runs[0].ownership_state, "orphaned_live_thread");
	assert_eq!(snapshot.worktrees[0].ownership, "post_review_lane");
	assert_eq!(
		snapshot.worktrees[0].ownership_reason,
		"Review & Landing owns this worktree as `wait_for_review`."
	);
	assert_eq!(snapshot.worktrees[0].recovery_next_action, None);
	assert!(!snapshot.worktrees[0].provenance.audit_required);
	assert_eq!(project.post_review_lane_count, 1);
	assert_eq!(project.retained_worktree_count, 1);
	assert!(rendered.contains("role: post_review_lane"));
	assert!(rendered.contains("recovery_next_action: none"));
	assert!(!rendered.contains("role: orphaned_live_thread"));
}
