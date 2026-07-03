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

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&closeout_issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
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
