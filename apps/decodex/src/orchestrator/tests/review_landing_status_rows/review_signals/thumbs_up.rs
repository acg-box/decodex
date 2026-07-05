use crate::{
	orchestrator::{
		self, EXTERNAL_REVIEW_PASS_PHRASE, ReviewOrchestrationMarker, StateStore, tests,
		tests::{
			FakePullRequestReviewStateInspector, FakeTracker,
			TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID, TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT,
		},
	},
	test_support,
};

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
