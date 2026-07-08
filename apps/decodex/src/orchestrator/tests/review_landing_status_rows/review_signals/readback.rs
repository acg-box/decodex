use std::fs;

use crate::{
	orchestrator::{
		self, ReviewLevel, StateStore, tests,
		tests::{FakePullRequestReviewStateInspector, FakeTracker},
	},
	state::ReviewPolicyCheckpointInput,
	test_support,
};

#[test]
fn preserves_authority_when_pr_readback_fails() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let head_oid = String::from_utf8(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(&repo_root)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Err(color_eyre::eyre::eyre!(
			"gh api failed"
		))]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].issue_identifier, issue.identifier);
	assert_eq!(lanes[0].classification, "wait_for_review");
	assert_eq!(lanes[0].reason, "pull_request_state_read_failed");
	assert_eq!(lanes[0].branch_name, "main");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
	assert_eq!(lanes[0].pr_head_sha.as_deref(), Some(head_oid.as_str()));
	assert_eq!(lanes[0].readback_warning.as_deref(), Some("pull_request_state_read_failed"));
	assert_eq!(lanes[0].readback_root_cause.as_deref(), Some("github_api_read_failed"));
	assert_eq!(lanes[0].pr_state, None);
}

#[test]
fn build_post_review_lane_statuses_skips_external_review_when_disabled() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_review_level(&config, ReviewLevel::Off);
	let repo_root = config.repo_root().to_path_buf();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let head_oid = String::from_utf8(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(&repo_root)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);

	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);
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
	assert_eq!(lanes[0].reason, "non_github_review_ready_to_land");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
	assert!(
		lanes[0].loop_status.as_ref().and_then(|status| status.review.as_ref()).is_none(),
		"ready_to_land must not project a pending review checkpoint"
	);
}

#[test]
fn standard_review_waits_for_runtime_review_checkpoint_before_landing() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_review_level(&config, ReviewLevel::Standard);
	let repo_root = config.repo_root().to_path_buf();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let head_oid = String::from_utf8(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(&repo_root)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);

	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);
	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state.clone())]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "wait_for_review");
	assert_eq!(lanes[0].reason, "runtime_standard_review_checkpoint_pending");

	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: &issue.id,
			run_id: "runtime-review",
			attempt_number: 1,
			phase: "repair",
			review_level: "standard",
			status: "clean",
			head_sha: &head_oid,
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("runtime review checkpoint should persist");

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state.clone())]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "ready_to_land");
	assert_eq!(lanes[0].reason, "non_github_review_ready_to_land");

	fs::write(repo_root.join("dirty-after-review.txt"), "dirty\n")
		.expect("dirty worktree file should write");
	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "blocked");
	assert_eq!(lanes[0].reason, "runtime_standard_review_clean_checkpoint_worktree_dirty");
}
