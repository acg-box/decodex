use crate::{
	agent::{ReviewExecutionMode, ReviewHandoffContext, TrackerToolBridge},
	config::ReviewLevel,
	orchestrator::{
		IssueDispatchMode, IssueTurnContinuationGuard, TurnContinuationGuard,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker, TEST_SERVICE_ID,
			intake_run_and_prompting,
		},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn continuation_guard_allows_closeout_continuation_after_issue_reaches_completed_state() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = intake_run_and_prompting::run_and_prompting_service_owned_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/175";

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let tracker_tool_bridge = TrackerToolBridge::with_run_context_and_state_store(
		&tracker,
		&issue,
		&workflow,
		ReviewHandoffContext {
			attempt_number: 1,
			branch_name: worktree.branch_name.clone(),
			run_id: String::from("run-closeout-continuation"),
			service_id: String::from(TEST_SERVICE_ID),
			worktree_path: worktree.path.display().to_string(),
			cwd: worktree.path.clone(),
			github_token_env_var: None,
			github_command_path: None,
			review_level: ReviewLevel::Strict,
			mode: ReviewExecutionMode::Closeout,
			recorded_pr_url: Some(String::from(pr_url)),
		},
		&state_store,
	);
	let mut review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	review_state.state = String::from("MERGED");

	let review_state_inspector =
		FakePullRequestReviewStateInspector::new(vec![Ok(review_state.clone()), Ok(review_state)]);
	let guard = IssueTurnContinuationGuard {
		tracker: &tracker,
		tracker_tool_bridge: &tracker_tool_bridge,
		workflow: &workflow,
		service_id: TEST_SERVICE_ID,
		issue_id: &issue.id,
		issue_identifier: &issue.identifier,
		initial_issue_state: "In Review",
		retry_project_slug: issue
			.project_slug
			.as_deref()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Closeout,
		review_state_inspector: Some(&review_state_inspector),
	};

	assert!(
		guard
			.should_continue_turn(2)
			.expect("retained closeout lane should continue while the issue remains completed")
	);

	guard
		.validate_continuation_boundary(2)
		.expect("closeout continuation boundary should stay valid after tracker completion");
}

#[test]
fn continuation_guard_blocks_closeout_continuation_when_completed_issue_pr_is_open() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = intake_run_and_prompting::run_and_prompting_service_owned_issue("Done");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/176";

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let tracker_tool_bridge = TrackerToolBridge::with_run_context_and_state_store(
		&tracker,
		&issue,
		&workflow,
		ReviewHandoffContext {
			attempt_number: 1,
			branch_name: worktree.branch_name.clone(),
			run_id: String::from("run-closeout-open-pr"),
			service_id: String::from(TEST_SERVICE_ID),
			worktree_path: worktree.path.display().to_string(),
			cwd: worktree.path.clone(),
			github_token_env_var: None,
			github_command_path: None,
			review_level: ReviewLevel::Strict,
			mode: ReviewExecutionMode::Closeout,
			recorded_pr_url: Some(String::from(pr_url)),
		},
		&state_store,
	);
	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);
	let review_state_inspector =
		FakePullRequestReviewStateInspector::new(vec![Ok(review_state.clone()), Ok(review_state)]);
	let guard = IssueTurnContinuationGuard {
		tracker: &tracker,
		tracker_tool_bridge: &tracker_tool_bridge,
		workflow: &workflow,
		service_id: TEST_SERVICE_ID,
		issue_id: &issue.id,
		issue_identifier: &issue.identifier,
		initial_issue_state: "In Review",
		retry_project_slug: issue
			.project_slug
			.as_deref()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Closeout,
		review_state_inspector: Some(&review_state_inspector),
	};

	assert!(
		!guard.should_continue_turn(2).expect(
			"completed issues must not continue retained closeout while the PR is still open"
		)
	);

	let error = guard
		.validate_continuation_boundary(2)
		.expect_err("completed issues with open PRs must fail the retained closeout boundary");

	assert!(error.to_string().contains("retained closeout continuation boundary"));
}
