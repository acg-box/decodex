use std::{
	fs,
	process::{self, Command},
};

use color_eyre::{Report, eyre};
use tempfile::TempDir;

use crate::{
	orchestrator::{
		self, GhPullRequestReviewStateInspector, PostReviewLaneClassification,
		PostReviewLaneDecision, PostReviewLaneSnapshot, PostReviewReadbackDegradation,
		PullRequestActor, PullRequestIssueCommentConnection, PullRequestIssueCommentNode,
		PullRequestIssueCommentsNode, PullRequestPageInfo, PullRequestReadbackFailure,
		PullRequestReadbackRootCause, PullRequestRepository, PullRequestRepositoryOwner,
		PullRequestReviewStateNode, StateStore, tests,
		tests::{FakePullRequestReviewStateInspector, TEST_SERVICE_ID},
	},
	test_support::TestEnvVarGuard,
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

#[test]
fn classify_post_review_lane_degrades_pull_request_state_read_failures_to_handoff_marker() {
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
		&FakePullRequestReviewStateInspector::new(vec![Err(color_eyre::eyre::eyre!(
			"gh api failed"
		))]),
	)
	.expect("classification should preserve handoff marker readback");

	assert_eq!(classification.decision, PostReviewLaneDecision::WaitForReview);
	assert_eq!(classification.reason, "pull_request_state_read_failed");
	assert_eq!(
		classification.pr_url.as_deref(),
		Some("https://github.com/hack-ink/decodex/pull/174")
	);
	assert_eq!(classification.pr_head_sha.as_deref(), Some(head_oid.as_str()));
	assert_eq!(classification.readback_warning.as_deref(), Some("pull_request_state_read_failed"));
	assert_eq!(classification.readback_root_cause.as_deref(), Some("github_api_read_failed"));
}

#[test]
fn post_review_readback_degradation_helper_preserves_warning_and_typed_cause() {
	let marker_head_oid = "1111111111111111111111111111111111111111";
	let review_head_oid = "2222222222222222222222222222222222222222";
	let pr_url = "https://github.com/hack-ink/decodex/pull/174";
	let review_handoff =
		tests::sample_review_handoff_marker("x/pubfi-pub-101", pr_url, marker_head_oid);
	let pull_request_readback = PostReviewReadbackDegradation::pull_request_state_from_handoff(
		&review_handoff,
		PullRequestReadbackRootCause::GithubApiReadFailed,
	)
	.wait_for_review_classification(None);

	assert_eq!(pull_request_readback.decision, PostReviewLaneDecision::WaitForReview);
	assert_eq!(pull_request_readback.reason, "pull_request_state_read_failed");
	assert_eq!(
		pull_request_readback.readback_warning.as_deref(),
		Some("pull_request_state_read_failed")
	);
	assert_eq!(
		pull_request_readback.readback_root_cause.as_deref(),
		Some("github_api_read_failed")
	);
	assert_eq!(pull_request_readback.pr_url.as_deref(), Some(pr_url));
	assert_eq!(pull_request_readback.pr_head_sha.as_deref(), Some(marker_head_oid));
	assert_eq!(pull_request_readback.pr_state, None);

	let tracker_readback =
		PostReviewReadbackDegradation::tracker_issue_from_handoff(&review_handoff)
			.wait_for_review_classification(Some(tests::sample_pull_request_review_state(
				pr_url,
				"x/pubfi-pub-101",
				review_head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"CLEAN",
				Some("SUCCESS"),
				2,
			)));

	assert_eq!(tracker_readback.decision, PostReviewLaneDecision::WaitForReview);
	assert_eq!(tracker_readback.reason, "tracker_issue_readback_degraded");
	assert_eq!(
		tracker_readback.readback_warning.as_deref(),
		Some("tracker_issue_readback_degraded")
	);
	assert_eq!(
		tracker_readback.readback_root_cause.as_deref(),
		Some("tracker_issue_readback_failed")
	);
	assert_eq!(tracker_readback.pr_head_sha.as_deref(), Some(review_head_oid));
	assert_eq!(tracker_readback.pr_state.as_deref(), Some("OPEN"));
	assert_eq!(tracker_readback.review_decision.as_deref(), Some("APPROVED"));
	assert_eq!(tracker_readback.check_state.as_deref(), Some("SUCCESS"));
	assert_eq!(tracker_readback.unresolved_review_threads, Some(2));
}

#[test]
fn classify_post_review_lane_degrades_missing_or_blank_github_token_env_var() {
	let missing = classify_post_review_lane_with_github_token_env_var(None);

	assert_eq!(missing.decision, PostReviewLaneDecision::WaitForReview);
	assert_eq!(missing.reason, "pull_request_state_read_failed");
	assert_eq!(missing.pr_url.as_deref(), Some("https://github.com/hack-ink/decodex/pull/174"));
	assert_eq!(missing.readback_warning.as_deref(), Some("pull_request_state_read_failed"));
	assert_eq!(missing.readback_root_cause.as_deref(), Some("missing_github_token"));

	let env_var = format!("DECODEX_TEST_BLANK_STATUS_GITHUB_TOKEN_ENV_{}", process::id());
	let _env_guard = TestEnvVarGuard::set(&env_var, "");
	let blank = classify_post_review_lane_with_github_token_env_var(Some(env_var));

	assert_eq!(blank.decision, PostReviewLaneDecision::WaitForReview);
	assert_eq!(blank.reason, "pull_request_state_read_failed");
	assert_eq!(blank.pr_url.as_deref(), Some("https://github.com/hack-ink/decodex/pull/174"));
	assert_eq!(blank.readback_warning.as_deref(), Some("pull_request_state_read_failed"));
	assert_eq!(blank.readback_root_cause.as_deref(), Some("missing_github_token"));
}

#[test]
fn pull_request_readback_root_cause_classifier_maps_cli_and_shape_failures() {
	let missing_cli_error = Command::new("decodex-test-missing-gh-command")
		.output()
		.expect_err("missing command should fail with an io error");
	let missing_cli = PullRequestReadbackFailure::from_report(Report::from(missing_cli_error));
	let shape_failure = PullRequestReadbackFailure::from_report(eyre::eyre!(
		"GitHub GraphQL response for `https://github.com/hack-ink/decodex/pull/174` did not include a pull request."
	));

	assert_eq!(
		missing_cli.root_cause(),
		orchestrator::PullRequestReadbackRootCause::MissingGithubCli
	);
	assert_eq!(
		shape_failure.root_cause(),
		orchestrator::PullRequestReadbackRootCause::PullRequestShapeReadFailed
	);
}

fn classify_post_review_lane_with_github_token_env_var(
	github_token_env_var: Option<String>,
) -> PostReviewLaneClassification {
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
		local_head_oid: Some(head_oid),
	};

	orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&GhPullRequestReviewStateInspector { github_token_env_var, github_command_path: None },
	)
	.expect("classification should degrade to blocked")
}

#[test]
fn merge_pull_request_review_state_page_counts_unresolved_threads_across_pages() {
	let first_page = tests::sample_pull_request_review_state_page(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		100,
		true,
		Some("cursor-1"),
	);
	let repository = tests::sample_pull_request_review_state_repository(first_page);
	let mut review_state = orchestrator::pull_request_review_state_from_page(
		&repository,
		repository.pull_request.as_ref().expect("pull request should exist"),
	)
	.expect("review state should build");
	let next_page = tests::sample_pull_request_review_state_page(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		1,
		false,
		None,
	);
	let next_repository = tests::sample_pull_request_review_state_repository(next_page);
	let next_cursor = orchestrator::merge_pull_request_review_state_page(
		&mut review_state,
		&next_repository,
		next_repository.pull_request.as_ref().expect("pull request should exist"),
	)
	.expect("page merge should succeed");

	assert_eq!(review_state.unresolved_review_threads, 101);
	assert_eq!(next_cursor, None);
}

#[test]
fn merge_pull_request_issue_comment_page_appends_comments_across_pages() {
	let first_page = tests::sample_pull_request_review_state_page(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		0,
		false,
		None,
	);
	let repository = tests::sample_pull_request_review_state_repository(first_page);
	let mut review_state = orchestrator::pull_request_review_state_from_page(
		&repository,
		repository.pull_request.as_ref().expect("pull request should exist"),
	)
	.expect("review state should build");
	let next_page = PullRequestIssueCommentsNode {
		url: String::from("https://github.com/hack-ink/decodex/pull/174"),
		comments: PullRequestIssueCommentConnection {
			nodes: vec![PullRequestIssueCommentNode {
				database_id: 501,
				body: String::from("Looks good"),
				created_at: String::from("2025-11-03T00:00:00Z"),
				author: Some(PullRequestActor {
					login: String::from(crate::orchestrator::EXTERNAL_REVIEW_ACTOR_LOGIN),
				}),
				reaction_groups: Vec::new(),
			}],
			page_info: PullRequestPageInfo { has_next_page: false, end_cursor: None },
		},
	};
	let next_cursor =
		orchestrator::merge_pull_request_issue_comment_page(&mut review_state, &next_page)
			.expect("comment page merge should succeed");

	assert_eq!(review_state.issue_comments.len(), 1);
	assert_eq!(review_state.issue_comments[0].database_id, 501);
	assert_eq!(next_cursor, None);
}

#[test]
fn merge_pull_request_review_state_page_rejects_changed_metadata_across_pages() {
	type ReviewPageMutation = fn(&mut PullRequestReviewStateNode);

	let cases: [(&str, ReviewPageMutation); 4] = [
		("review metadata", |page| {
			page.review_decision = Some(String::from("CHANGES_REQUESTED"));
		}),
		("pending review request count", |page| {
			page.review_requests.total_count = 1;
		}),
		("head repository owner", |page| {
			page.head_repository_owner =
				Some(PullRequestRepositoryOwner { login: String::from("someone-else") });
		}),
		("head repository name", |page| {
			page.head_repository =
				Some(PullRequestRepository { name: String::from("decodex-fork") });
		}),
	];

	for (case_name, mutate) in cases {
		assert_review_state_page_rejects_changed_metadata(case_name, mutate);
	}
}

fn assert_review_state_page_rejects_changed_metadata(
	case_name: &str,
	mutate: fn(&mut PullRequestReviewStateNode),
) {
	let first_page = tests::sample_pull_request_review_state_page(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		100,
		true,
		Some("cursor-1"),
	);
	let repository = tests::sample_pull_request_review_state_repository(first_page);
	let mut review_state = orchestrator::pull_request_review_state_from_page(
		&repository,
		repository.pull_request.as_ref().expect("pull request should exist"),
	)
	.expect("review state should build");
	let mut next_page = tests::sample_pull_request_review_state_page(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		1,
		false,
		None,
	);

	mutate(&mut next_page);

	let next_repository = tests::sample_pull_request_review_state_repository(next_page);
	let error = orchestrator::merge_pull_request_review_state_page(
		&mut review_state,
		&next_repository,
		next_repository.pull_request.as_ref().expect("pull request should exist"),
	)
	.expect_err("changed metadata should fail");

	assert!(error.to_string().contains("changed while paginating"), "{case_name}");
}

#[test]
fn pull_request_review_state_query_requests_required_fields() {
	for expected_fragment in [
		"mergeCommitAllowed",
		"headRepository {\n        name\n      }",
		"comments(first: 100) {\n        nodes {\n          databaseId",
		"pageInfo {\n          hasNextPage\n          endCursor\n        }",
	] {
		assert!(
			orchestrator::PULL_REQUEST_REVIEW_STATE_QUERY.contains(expected_fragment),
			"query should include {expected_fragment}"
		);
	}
}

#[test]
fn next_pull_request_review_threads_cursor_requires_end_cursor_when_pagination_continues() {
	let page = tests::sample_pull_request_review_state_page(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		100,
		true,
		None,
	);
	let error = orchestrator::next_pull_request_review_threads_cursor(&page)
		.expect_err("missing end cursor should fail");

	assert!(error.to_string().contains("without an end cursor"));
}

#[test]
fn next_pull_request_issue_comments_cursor_requires_end_cursor_when_pagination_continues() {
	let comments = PullRequestIssueCommentConnection {
		nodes: Vec::new(),
		page_info: PullRequestPageInfo { has_next_page: true, end_cursor: None },
	};
	let error = orchestrator::next_pull_request_issue_comments_cursor(
		&comments,
		"https://github.com/hack-ink/decodex/pull/174",
	)
	.expect_err("missing end cursor should fail");

	assert!(error.to_string().contains("without an end cursor"));
}
