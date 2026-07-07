use std::fs;

use crate::{
	orchestrator::{
		self, StateStore, tests,
		tests::{FakePullRequestReviewStateInspector, FakeTracker},
	},
	worktree::WorktreeManager,
};

#[test]
fn build_post_review_lane_statuses_blocks_review_handoff_lineage_rewrite() {
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
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	tests::git_status_success(&worktree.path, &["checkout", "--orphan", "rewrite-history"]);
	fs::write(worktree.path.join("rewrite.txt"), "rewritten history\n")
		.expect("rewrite file should write");
	tests::git_status_success(&worktree.path, &["add", "rewrite.txt"]);
	tests::git_status_success(&worktree.path, &["commit", "-m", "rewrite history"]);
	tests::git_status_success(&worktree.path, &["branch", "-M", &worktree.branch_name]);

	let rewritten_head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);

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

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				pr_url,
				&worktree.branch_name,
				&rewritten_head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"CLEAN",
				Some("SUCCESS"),
				0,
			),
		)]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "blocked");
	assert_eq!(lanes[0].reason, "lifecycle_record_lineage_mismatch");
	assert_eq!(lanes[0].readback_root_cause.as_deref(), Some("lineage_validation_failed"));
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
}
