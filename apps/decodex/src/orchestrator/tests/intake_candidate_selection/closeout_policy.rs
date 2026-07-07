use crate::{
	orchestrator::{
		self, IssueDispatchMode, ReviewLifecycleHandoffFixture, TargetIssueRunContext,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker,
			intake_candidate_selection::support,
		},
	},
	state::StateStore,
	workflow::WorkflowDocument,
	worktree::WorktreeManager,
};

#[test]
fn candidate_selection_skips_issue_claimed_by_another_process() {
	let workflow = WorkflowDocument::parse_markdown(&tests::sample_workflow_markdown(
		"pubfi",
		&[],
		"Claim-aware workflow policy.\n",
		1,
	))
	.expect("workflow should parse");
	let (_temp_dir, config, _default_workflow) = tests::temp_project_layout();
	let remote_store = StateStore::open_in_memory().expect("remote state store should open");
	let local_store = StateStore::open_in_memory().expect("local state store should open");
	let claimed_issue = tests::sample_issue_with_sort_fields(
		"issue-claimed",
		"PUB-100",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let free_issue = tests::sample_issue_with_sort_fields(
		"issue-free",
		"PUB-101",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:16:18.133Z",
	);

	remote_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("remote dispatch-slot root should configure");
	local_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("local dispatch-slot root should configure");

	assert!(
		remote_store
			.try_acquire_lease(config.service_id(), &claimed_issue.id, "run-claimed", "In Progress")
			.expect("remote issue claim should succeed")
	);

	let tracker = FakeTracker::new(vec![claimed_issue.clone(), free_issue.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![claimed_issue, free_issue.clone()],
		&workflow,
		&local_store,
		config.service_id(),
	)
	.expect("candidate selection should succeed")
	.expect("the unclaimed issue should still be selected");

	assert_eq!(selected.id, free_issue.id);
}

#[test]
fn closeout_dispatch_policy_rejects_open_pull_request() {
	for state_name in ["Done", "In Review"] {
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let closeout_issue = support::candidate_selection_service_owned_issue(state_name);
		let tracker = FakeTracker::new(vec![closeout_issue.clone()]);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let worktree_manager =
			WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
		let worktree = worktree_manager
			.ensure_worktree(&closeout_issue.identifier, false)
			.expect("worktree should exist");
		let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
		let pr_url = "https://github.com/hack-ink/decodex/pull/176";

		tests::seed_review_lifecycle_handoff_fixture(
			&state_store,
			config.service_id(),
			&closeout_issue.id,
			&worktree.branch_name,
			pr_url,
			&head_oid,
		);

		let open_pr_review_state = tests::sample_pull_request_review_state(
			pr_url,
			&worktree.branch_name,
			&head_oid,
			Some("APPROVED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		);
		let dispatch_inspector =
			FakePullRequestReviewStateInspector::new(vec![Ok(open_pr_review_state.clone())]);
		let block_reason_inspector =
			FakePullRequestReviewStateInspector::new(vec![Ok(open_pr_review_state)]);

		assert!(
			!orchestrator::issue_passes_closeout_dispatch_policy_with_inspector(
				&tracker,
				&closeout_issue,
				&config,
				&workflow,
				&state_store,
				&dispatch_inspector,
			)
			.expect("dispatch policy inspection should succeed"),
			"{state_name} closeout issues must wait until the retained PR is merged",
		);
		assert_eq!(
			orchestrator::closeout_dispatch_block_reason_with_inspector(
				&tracker,
				&closeout_issue,
				&config,
				&workflow,
				&state_store,
				&block_reason_inspector,
			)
			.expect("block reason inspection should succeed"),
			Some("pull_request_not_merged"),
			"{state_name} closeout issues with open PRs should stay blocked, not ineligible",
		);
	}
}

#[test]
fn closeout_dispatch_policy_blocks_completed_issue_with_missing_review_handoff_record() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let closeout_issue = support::candidate_selection_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![closeout_issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&closeout_issue.identifier, false)
		.expect("worktree should exist");

	state_store
		.upsert_worktree(
			config.service_id(),
			&closeout_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("retained closeout worktree mapping should persist");

	assert!(
		!orchestrator::issue_passes_closeout_dispatch_policy(
			&tracker,
			&closeout_issue,
			&config,
			&workflow,
			&state_store,
		)
		.expect("dispatch policy inspection should succeed"),
		"completed issues with missing review handoff must remain non-dispatchable",
	);
	assert_eq!(
		orchestrator::closeout_dispatch_block_reason(
			&tracker,
			&closeout_issue,
			&config,
			&workflow,
			&state_store,
		)
		.expect("block reason inspection should succeed"),
		Some("missing_review_handoff_record"),
		"completed issues with retained worktrees but no review handoff should stay retained as blocked lanes",
	);
}

#[test]
fn closeout_dispatch_policy_rejects_completed_issue_without_service_active_label() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let closeout_issue = tests::sample_issue("Done", &[]);
	let tracker = FakeTracker::new(vec![closeout_issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&closeout_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/177";

	tests::seed_review_lifecycle_handoff_fixture(
		&state_store,
		config.service_id(),
		&closeout_issue.id,
		&worktree.branch_name,
		pr_url,
		&head_oid,
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

	let inspector = FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]);

	assert!(
		!orchestrator::issue_passes_closeout_dispatch_policy_with_inspector(
			&tracker,
			&closeout_issue,
			&config,
			&workflow,
			&state_store,
			&inspector,
		)
		.expect("dispatch policy inspection should succeed"),
		"completed issues without service ownership must not pass closeout dispatch",
	);
	assert_eq!(
		orchestrator::closeout_dispatch_block_reason_with_inspector(
			&tracker,
			&closeout_issue,
			&config,
			&workflow,
			&state_store,
			&FakePullRequestReviewStateInspector::new(Vec::new()),
		)
		.expect("block reason inspection should succeed"),
		None,
		"ownership-gated closeout issues should become ineligible rather than retained as blocked lanes",
	);
}

#[test]
fn closeout_dispatch_policy_allows_completed_issue_after_pull_request_merges() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let closeout_issue = support::candidate_selection_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![closeout_issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&closeout_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/177";

	tests::seed_review_lifecycle_handoff_fixture(
		&state_store,
		config.service_id(),
		&closeout_issue.id,
		&worktree.branch_name,
		pr_url,
		&head_oid,
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

	let inspector = FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]);

	assert!(
		orchestrator::issue_passes_closeout_dispatch_policy_with_inspector(
			&tracker,
			&closeout_issue,
			&config,
			&workflow,
			&state_store,
			&inspector,
		)
		.expect("dispatch policy inspection should succeed"),
		"completed issues should pass closeout dispatch after the retained PR merges",
	);
}

#[test]
fn closeout_dispatch_policy_uses_matching_handoff_record_for_current_branch() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let closeout_issue = support::candidate_selection_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![closeout_issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&closeout_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let current_pr_url = "https://github.com/hack-ink/decodex/pull/177";

	tests::seed_review_lifecycle_handoff_fixture(
		&state_store,
		config.service_id(),
		&closeout_issue.id,
		&worktree.branch_name,
		current_pr_url,
		&head_oid,
	);

	state_store
		.upsert_review_lifecycle_handoff_fixture(
			config.service_id(),
			&closeout_issue.id,
			&ReviewLifecycleHandoffFixture::new(
				String::from("run-review-handoff-newer"),
				2,
				String::from("x/pubfi-pub-101-next"),
				String::from("https://github.com/hack-ink/decodex/pull/999"),
				String::from("release/9.x"),
				String::from("x/pubfi-pub-101-next"),
				String::from("feedface"),
			),
		)
		.expect("unrelated branch handoff should persist");

	let mut merged_review_state = tests::sample_pull_request_review_state(
		current_pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	merged_review_state.state = String::from("MERGED");

	assert!(
		orchestrator::issue_passes_closeout_dispatch_policy_with_inspector(
			&tracker,
			&closeout_issue,
			&config,
			&workflow,
			&state_store,
			&FakePullRequestReviewStateInspector::new(vec![Ok(merged_review_state)]),
		)
		.expect("dispatch policy inspection should succeed"),
		"matching branch handoff records should remain dispatchable even when newer tracker comments belong to another branch",
	);
}

#[test]
fn non_dry_run_closeout_dispatch_errors_when_pr_state_read_fails() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_DIRECT_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
	let issue = support::candidate_selection_service_owned_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/179";

	tests::seed_review_lifecycle_handoff_fixture_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);

	let error = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &issue.id,
		preferred_issue_state: None,
		preferred_initial_issue_state: Some("In Review"),
		dry_run: false,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect_err("non-dry-run closeout dispatch should surface GH state read failures");

	assert!(error.to_string().contains("pull_request_state_read_failed"));
}

#[test]
fn post_review_closeout_selection_skips_completed_issue_with_open_pull_request() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let closeout_issue = support::candidate_selection_service_owned_issue("Done");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![closeout_issue.clone()],
		vec![vec![closeout_issue.clone()]],
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&closeout_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/175";

	state_store
		.upsert_worktree(
			config.service_id(),
			&closeout_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_value(
		&state_store,
		config.service_id(),
		&closeout_issue.id,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);

	let open_pr_review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);
	let inspector = FakePullRequestReviewStateInspector::new(vec![
		Ok(open_pr_review_state.clone()),
		Ok(open_pr_review_state),
	]);
	let selected = orchestrator::select_post_review_closeout_issue_candidate_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&[],
		&inspector,
	)
	.expect("post-review closeout selection should succeed");

	assert!(
		selected.is_none(),
		"completed issues should not auto-dispatch closeout until the PR is merged"
	);
}
