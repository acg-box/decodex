use crate::{
	orchestrator::{
		self,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker,
			intake_candidate_selection::support,
		},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

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

		tests::seed_review_handoff_marker(
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

	tests::seed_review_handoff_marker(
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
