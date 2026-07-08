use std::fs;

use tempfile::TempDir;

use crate::{
	orchestrator::{
		self, PostReviewLaneDecision, PostReviewLaneSnapshot, StateStore, tests,
		tests::FakePullRequestReviewStateInspector,
	},
	worktree::WorktreeManager,
};

#[test]
fn classify_post_review_lane_blocks_stale_review_handoff_head_without_lineage_proof() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let marker_head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let current_head_oid = String::from("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees("pubfi")
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		lifecycle_record: Some(tests::sample_review_lifecycle_record(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&marker_head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(current_head_oid.clone()),
	};
	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				"https://github.com/hack-ink/decodex/pull/174",
				"x/pubfi-pub-101",
				&current_head_oid,
				None,
				"MERGEABLE",
				"CLEAN",
				Some("SUCCESS"),
				0,
			),
		)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::Block);
	assert_eq!(classification.reason, "lifecycle_record_lineage_check_failed");
}

#[test]
fn blocks_when_pr_head_differs_from_worktree_head() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let branch_name = worktree.branch_name.clone();
	let worktree_path = worktree.path.clone();
	let current_head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_head_oid = String::from("feedfacefeedfacefeedfacefeedfacefeedface");
	let marker_head_oid = current_head_oid.clone();

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&branch_name,
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees(config.service_id())
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		lifecycle_record: Some(tests::sample_review_lifecycle_record(
			&branch_name,
			"https://github.com/hack-ink/decodex/pull/174",
			&marker_head_oid,
		)),
		local_branch_name: Some(branch_name.clone()),
		local_head_oid: Some(current_head_oid),
	};
	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				"https://github.com/hack-ink/decodex/pull/174",
				&branch_name,
				&pr_head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"CLEAN",
				Some("SUCCESS"),
				0,
			),
		)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::Block);
	assert_eq!(classification.reason, "pull_request_head_mismatch");
}
