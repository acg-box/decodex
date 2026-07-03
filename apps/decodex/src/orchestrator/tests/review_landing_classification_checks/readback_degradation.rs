use std::{
	fs,
	process::{self, Command},
};

use color_eyre::{Report, eyre};
use tempfile::TempDir;

use crate::{
	orchestrator::{
		self, PostReviewLaneDecision, PostReviewLaneSnapshot, PostReviewReadbackDegradation,
		PullRequestReadbackFailure, PullRequestReadbackRootCause, StateStore, tests,
		tests::FakePullRequestReviewStateInspector,
	},
	test_support::TestEnvVarGuard,
};

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
	let missing = super::classify_post_review_lane_with_github_token_env_var(None);

	assert_eq!(missing.decision, PostReviewLaneDecision::WaitForReview);
	assert_eq!(missing.reason, "pull_request_state_read_failed");
	assert_eq!(missing.pr_url.as_deref(), Some("https://github.com/hack-ink/decodex/pull/174"));
	assert_eq!(missing.readback_warning.as_deref(), Some("pull_request_state_read_failed"));
	assert_eq!(missing.readback_root_cause.as_deref(), Some("missing_github_token"));

	let env_var = format!("DECODEX_TEST_BLANK_STATUS_GITHUB_TOKEN_ENV_{}", process::id());
	let _env_guard = TestEnvVarGuard::set(&env_var, "");
	let blank = super::classify_post_review_lane_with_github_token_env_var(Some(env_var));

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
