use crate::{
	orchestrator::{
		self, StateStore, tests,
		tests::{FakePullRequestReviewStateInspector, FakeTracker},
	},
	worktree::WorktreeManager,
};

#[test]
fn build_post_review_lane_statuses_allows_descendant_review_handoff_head_after_repair_push() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let marker_head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let current_head_oid =
		tests::commit_worktree_change(&worktree.path, "repair.txt", "repair push\n", "repair push");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &marker_head_oid),
	);
	tests::seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_orchestration_marker(
			&worktree.branch_name,
			pr_url,
			&current_head_oid,
			"waiting_for_result",
			1,
		),
	);

	let mut review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&current_head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);
	tests::add_external_review_pass(&mut review_state);

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "ready_to_land");
	assert_eq!(lanes[0].reason, "external_review_passed_strict");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
}
