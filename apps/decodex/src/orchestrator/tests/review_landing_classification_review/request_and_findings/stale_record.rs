use std::fs;

use tempfile::TempDir;

use crate::orchestrator::{
	self, PostReviewLaneDecision, PostReviewLaneSnapshot, ReviewOrchestrationMarker, StateStore,
	tests::{
		self, FakePullRequestReviewStateInspector, TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID,
		TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT,
	},
};

#[test]
fn classify_post_review_lane_ignores_stale_review_orchestration_record_from_prior_handoff() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
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

	state_store
		.upsert_review_orchestration_marker(
			"pubfi",
			&issue.id,
			&ReviewOrchestrationMarker::new(
				"run-0",
				7,
				"x/pubfi-pub-101",
				"https://github.com/hack-ink/decodex/pull/99",
				"deadbeef",
				"waiting_for_result",
				Some(TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID),
				Some(TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT),
				Some(1),
				2,
				3,
				None,
			),
		)
		.expect("stale review orchestration marker should persist");

	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		lifecycle_record: Some(tests::sample_review_lifecycle_record(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};
	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				"https://github.com/hack-ink/decodex/pull/174",
				"x/pubfi-pub-101",
				&head_oid,
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
	assert_eq!(classification.reason, "review_orchestration_pr_mismatch");
}
