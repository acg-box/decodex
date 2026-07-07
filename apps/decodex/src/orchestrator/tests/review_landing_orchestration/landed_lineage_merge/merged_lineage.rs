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
fn reconcile_post_review_orchestration_skips_merged_landed_lineage_without_manual_attention() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let pr_head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let merge_commit_oid = tests::commit_worktree_change(
		&worktree.path,
		"landed.txt",
		"landed\n",
		"land retained lane",
	);
	let current_head_oid =
		tests::commit_worktree_change(&worktree.path, "later.txt", "later\n", "advance main later");
	let pr_url = "https://github.com/hack-ink/decodex/pull/203";

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
		&tests::sample_review_lifecycle_handoff_fixture(
			&worktree.branch_name,
			pr_url,
			&pr_head_oid,
		),
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
			&current_head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

	let mut review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&pr_head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	review_state.state = String::from("MERGED");
	review_state.merge_commit_oid = Some(merge_commit_oid);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("merged post-review orchestration should not fail");

	let marker = tests::persisted_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
	);

	assert_eq!(marker.phase(), "request_pending");
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.state_updates.borrow().is_empty());
}
