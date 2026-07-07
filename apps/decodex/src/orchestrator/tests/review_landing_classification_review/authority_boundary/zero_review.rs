use std::fs;

use tempfile::TempDir;

use crate::orchestrator::{
	self, PostReviewLaneDecision, PostReviewLaneSnapshot, StateStore,
	tests::{
		self, FakePullRequestReviewStateInspector, TEST_SERVICE_ID,
		review_landing_classification_review,
	},
};

#[test]
fn classify_post_review_lane_ready_to_land_allows_zero_required_review_repos() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	review_landing_classification_review::initialize_empty_git_worktree(&worktree_path);

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
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};

	tests::seed_review_lifecycle_transition_fixture(
		&state_store,
		TEST_SERVICE_ID,
		&snapshot.issue.id,
		&tests::sample_review_lifecycle_transition_fixture(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"waiting_for_result",
			1,
		),
	);

	let mut review_state = tests::sample_pull_request_review_state(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		&head_oid,
		None,
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);
	tests::add_external_review_pass(&mut review_state);
	super::super::record_clean_review_checkpoint_for_head(
		&state_store,
		&snapshot.issue.id,
		&head_oid,
	);

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::ReadyToLand);
	assert_eq!(classification.reason, "external_review_passed_strict");
}
