use std::fs;

use crate::{
	orchestrator::{
		self, ReviewLifecycleTransitionFixture, StateStore,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker, review_landing_status_support,
		},
	},
	worktree::WorktreeManager,
};

#[test]
fn reconcile_post_review_orchestration_waits_when_worktree_head_read_fails() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let branch_ref_path =
		config.repo_root().join(".git").join("refs").join("heads").join(&worktree.branch_name);
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);
	tests::seed_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&ReviewLifecycleTransitionFixture::new(
			"run-1",
			1,
			&worktree.branch_name,
			pr_url,
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
	fs::remove_file(&branch_ref_path).expect("branch ref should remove");
	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(Vec::new()),
	)
	.expect("post-review orchestration should tolerate local worktree readback failure");

	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
	assert!(
		state_store
			.list_linear_execution_events(config.service_id(), &issue.id)
			.expect("linear execution events should list")
			.is_empty()
	);
}

#[test]
fn reconcile_post_review_orchestration_waits_when_worktree_branch_read_fails() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let missing_worktree_path = temp_dir.path().join("missing-retained-worktree");
	let branch_name = "x/pubfi-pub-101";
	let head_oid = tests::git_output(config.repo_root(), &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	fs::create_dir_all(&missing_worktree_path).expect("broken worktree path should exist");

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			branch_name,
			&missing_worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_lifecycle_handoff_fixture(branch_name, pr_url, &head_oid),
	);
	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(Vec::new()),
	)
	.expect("post-review orchestration should tolerate local branch readback failure");

	assert!(
		tracker.comments.borrow().is_empty(),
		"unexpected tracker comments: {:#?}",
		tracker.comments.borrow()
	);
	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
	assert!(
		state_store
			.list_linear_execution_events(config.service_id(), &issue.id)
			.expect("linear execution events should list")
			.is_empty()
	);
}
