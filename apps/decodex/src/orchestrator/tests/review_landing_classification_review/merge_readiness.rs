use std::fs;

use tempfile::TempDir;

use crate::orchestrator::{
	self, PostReviewLaneDecision, PostReviewLaneSnapshot, StateStore,
	tests::{self, FakePullRequestReviewStateInspector, TEST_SERVICE_ID},
};

#[test]
fn classify_post_review_lane_waits_when_external_review_request_is_still_pending() {
	for review_decision in [None, Some("APPROVED")] {
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

		tests::seed_review_orchestration_marker(
			&state_store,
			TEST_SERVICE_ID,
			&snapshot.issue.id,
			&tests::sample_review_orchestration_marker(
				"x/pubfi-pub-101",
				"https://github.com/hack-ink/decodex/pull/174",
				&head_oid,
				"waiting_for_ack",
				0,
			),
		);

		let mut review_state = tests::sample_pull_request_review_state_with_pending_requests(
			"https://github.com/hack-ink/decodex/pull/174",
			"x/pubfi-pub-101",
			&head_oid,
			review_decision,
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
			1,
		);

		tests::add_external_review_ack(&mut review_state);

		let classification = orchestrator::classify_post_review_lane(
			&snapshot,
			&state_store,
			&tests::sample_workflow(),
			&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
		)
		.expect("classification should succeed");

		assert_eq!(classification.decision, PostReviewLaneDecision::WaitForReview);
		assert_eq!(classification.reason, "external_review_result_pending");
	}
}

#[test]
fn classify_post_review_lane_continues_when_pull_request_is_already_merged() {
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
	let mut review_state = tests::sample_pull_request_review_state(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	review_state.state = String::from("MERGED");

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::Continue);
	assert_eq!(classification.reason, "pull_request_merged_closeout_pending");
}

#[test]
fn classify_post_review_lane_repairs_unmergeable_pr_before_review_waits() {
	let cases = [
		(
			"conflict before review required wait",
			Some("REVIEW_REQUIRED"),
			"CONFLICTING",
			"DIRTY",
			0,
			"pull_request_merge_conflict",
		),
		(
			"conflict before pending review wait",
			None,
			"CONFLICTING",
			"DIRTY",
			1,
			"pull_request_merge_conflict",
		),
		(
			"behind before review required wait",
			Some("REVIEW_REQUIRED"),
			"MERGEABLE",
			"BEHIND",
			0,
			"pull_request_branch_behind_base",
		),
		(
			"behind before pending review wait",
			None,
			"MERGEABLE",
			"BEHIND",
			1,
			"pull_request_branch_behind_base",
		),
	];

	for (case_name, review_decision, mergeable, merge_state, pending_requests, reason) in cases {
		let classification = super::classify_post_review_lane_with_pr_state(
			review_decision,
			mergeable,
			merge_state,
			Some("SUCCESS"),
			pending_requests,
		);

		assert_eq!(
			classification.decision,
			PostReviewLaneDecision::NeedsReviewRepair,
			"{case_name}"
		);
		assert_eq!(classification.reason, reason, "{case_name}");
	}
}
