use std::fs;

use tempfile::TempDir;

use crate::orchestrator::{
	self, PostReviewLaneDecision, PostReviewLaneSnapshot, ReviewOrchestrationMarker, StateStore,
	tests::{self, FakePullRequestReviewStateInspector, TEST_SERVICE_ID},
};

#[test]
fn classify_post_review_lane_request_pending_waits_for_green_checks_before_external_review_request()
{
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
		review_handoff: Some(tests::sample_review_handoff_marker(
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
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

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
				Some("PENDING"),
				0,
			),
		)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::WaitForReview);
	assert_eq!(classification.reason, "external_review_request_waiting_for_green_checks",);
}

#[test]
fn classify_post_review_lane_request_pending_routes_fixable_ci_red_to_repair() {
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
		review_handoff: Some(tests::sample_review_handoff_marker(
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
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

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
				"BLOCKED",
				Some("FAILURE"),
				0,
			),
		)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::NeedsReviewRepair);
	assert_eq!(classification.reason, "required_checks_failed");
}

#[test]
fn classify_post_review_lane_request_pending_repairs_unhandled_ci_red() {
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
		review_handoff: Some(tests::sample_review_handoff_marker(
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
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

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
				"UNSTABLE",
				Some("FAILURE"),
				0,
			),
		)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::NeedsReviewRepair);
	assert_eq!(classification.reason, "external_review_request_ci_red_repair_required",);
}

#[test]
fn classify_post_review_lane_request_pending_waits_for_unknown_check_state() {
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
		review_handoff: Some(tests::sample_review_handoff_marker(
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
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

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
				"UNSTABLE",
				Some("UNKNOWN_NEW_STATE"),
				0,
			),
		)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::WaitForReview);
	assert_eq!(classification.reason, "external_review_request_waiting_for_green_checks",);
}
