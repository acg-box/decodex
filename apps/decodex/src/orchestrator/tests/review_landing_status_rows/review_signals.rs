use crate::{
	orchestrator::{
		self, EXTERNAL_REVIEW_PASS_PHRASE, ReviewLevel, ReviewOrchestrationMarker, StateStore,
		tests,
		tests::{
			FakePullRequestReviewStateInspector, FakeTracker,
			TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID, TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT,
			TEST_NON_EXTERNAL_REVIEW_ACTOR_LOGIN,
		},
	},
	test_support,
};

#[test]
fn build_post_review_lane_statuses_preserves_handoff_marker_when_pr_readback_fails() {
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

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_handoff_marker("main", pr_url, &head_oid),
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

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_handoff_marker("main", pr_url, &head_oid),
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
fn build_post_review_lane_statuses_routes_mixed_external_pass_and_feedback_to_repair() {
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

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_handoff_marker("main", pr_url, &head_oid),
	);
	tests::seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_orchestration_marker(
			"main",
			pr_url,
			&head_oid,
			"waiting_for_result",
			1,
		),
	);

	let mut review_state = tests::sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);

	review_state.issue_description_external_review_thumbs_up_count = 1;

	tests::add_external_review_summary(
		&mut review_state,
		"Didn't find any major issues. Please fix X.",
		"COMMENTED",
		TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT + 1,
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
	assert_eq!(lanes[0].classification, "needs_review_repair");
	assert_eq!(lanes[0].reason, "external_review_feedback_pending_repair");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
}

#[test]
fn build_post_review_lane_statuses_ignores_non_external_review_signals() {
	for (phase, signal, expected_reason) in [
		("waiting_for_ack", "ack", "external_review_ack_pending"),
		("waiting_for_result", "pass", "external_review_result_pending"),
	] {
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

		tests::seed_review_handoff_marker_for_path(
			&state_store,
			config.service_id(),
			&repo_root,
			&tests::sample_review_handoff_marker("main", pr_url, &head_oid),
		);
		tests::seed_review_orchestration_marker_for_path(
			&state_store,
			config.service_id(),
			&repo_root,
			&tests::sample_review_orchestration_marker("main", pr_url, &head_oid, phase, 1),
		);

		let mut review_state = tests::sample_pull_request_review_state(
			pr_url,
			"main",
			&head_oid,
			Some("APPROVED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		);

		match signal {
			"ack" => tests::add_review_request_ack_from_actor(
				&mut review_state,
				TEST_NON_EXTERNAL_REVIEW_ACTOR_LOGIN,
			),
			"pass" => {
				tests::add_external_review_ack(&mut review_state);
				tests::add_external_review_pass_from_actor(
					&mut review_state,
					TEST_NON_EXTERNAL_REVIEW_ACTOR_LOGIN,
				);
			},

			_ => unreachable!("test case should use a known non-external signal"),
		}

		let lanes = orchestrator::build_post_review_lane_statuses(
			&tracker,
			&config,
			&workflow,
			&state_store,
			&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
		)
		.expect("post-review lane status build should succeed");

		assert_eq!(lanes.len(), 1);
		assert_eq!(lanes[0].classification, "wait_for_review");
		assert_eq!(lanes[0].reason, expected_reason);
		assert!(
			lanes[0].loop_status.as_ref().and_then(|status| status.review.as_ref()).is_none(),
			"wait_for_review must not project a repair review checkpoint"
		);
	}
}

#[test]
fn build_post_review_lane_statuses_accepts_existing_description_thumbs_up_for_later_pass_rounds() {
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

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_handoff_marker("main", pr_url, &head_oid),
	);
	tests::seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			"main",
			pr_url,
			&head_oid,
			"waiting_for_result",
			Some(TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID),
			Some(TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT),
			Some(1),
			0,
			1,
			None,
		),
	);

	let mut review_state = tests::sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);

	review_state.issue_description_external_review_thumbs_up_count = 1;

	tests::add_external_review_summary(
		&mut review_state,
		EXTERNAL_REVIEW_PASS_PHRASE,
		"APPROVED",
		TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT + 1,
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
	assert_eq!(lanes[0].reason, "external_review_passed_strict");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
}
