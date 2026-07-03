use std::fs;

use tempfile::TempDir;

use crate::orchestrator::{
	self, PostReviewLaneDecision, PostReviewLaneSnapshot, StateStore,
	tests::{self, FakePullRequestReviewStateInspector, TEST_SERVICE_ID},
};

#[test]
fn classify_post_review_lane_blocks_completed_issue_until_pull_request_is_merged() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("Done", &[]);
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
	assert_eq!(classification.reason, "issue_completed_before_pull_request_merged");
}

#[test]
fn classify_post_review_lane_waits_for_pending_required_checks_before_ready_to_land() {
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
		&tests::sample_review_orchestration_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"pass_waiting_for_gates",
			1,
		),
	);

	let mut review_state = tests::sample_pull_request_review_state(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"UNSTABLE",
		Some("PENDING"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);
	tests::add_external_review_pass(&mut review_state);

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::WaitForReview);
	assert_eq!(classification.reason, "external_review_passed_waiting_gates");
}

#[test]
fn classify_post_review_lane_routes_non_clean_landing_to_agent_fallback() {
	for (merge_state, status_check_state) in
		[("HAS_HOOKS", Some("SUCCESS")), ("UNSTABLE", Some("FAILURE"))]
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
			&tests::sample_review_orchestration_marker(
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
			Some("APPROVED"),
			"MERGEABLE",
			merge_state,
			status_check_state,
			0,
		);

		tests::add_external_review_ack(&mut review_state);
		tests::add_external_review_pass(&mut review_state);

		let classification = orchestrator::classify_post_review_lane(
			&snapshot,
			&state_store,
			&tests::sample_workflow(),
			&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
		)
		.expect("classification should succeed");

		assert_eq!(classification.decision, PostReviewLaneDecision::NeedsReviewRepair);
		assert_eq!(classification.reason, "retained_landing_agent_fallback_required");
	}
}

#[test]
fn classify_post_review_lane_waits_for_review_before_optional_failed_checks() {
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
		&tests::sample_review_orchestration_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"waiting_for_result",
			0,
		),
	);

	let mut review_state = tests::sample_pull_request_review_state(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		&head_oid,
		Some("REVIEW_REQUIRED"),
		"MERGEABLE",
		"UNSTABLE",
		Some("FAILURE"),
		0,
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

#[test]
fn classify_post_review_lane_requires_review_repair_before_review_when_required_checks_fail() {
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
	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				"https://github.com/hack-ink/decodex/pull/174",
				"x/pubfi-pub-101",
				&head_oid,
				Some("REVIEW_REQUIRED"),
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
fn classify_post_review_lane_blocks_checkout_branch_mismatch() {
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
		local_branch_name: Some(String::from("x/pubfi-pub-999")),
		local_head_oid: Some(head_oid),
	};
	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(Vec::new()),
	)
	.expect("classification should degrade to blocked");

	assert_eq!(classification.decision, PostReviewLaneDecision::Block);
	assert_eq!(classification.reason, "worktree_checkout_branch_mismatch");
}
