use crate::{
	orchestrator::{
		self, ReviewHandoffMarker,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker,
			intake_candidate_selection::support,
		},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

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

	tests::seed_review_handoff_marker(
		&state_store,
		config.service_id(),
		&closeout_issue.id,
		&worktree.branch_name,
		current_pr_url,
		&head_oid,
	);

	state_store
		.upsert_review_handoff_marker(
			config.service_id(),
			&closeout_issue.id,
			&ReviewHandoffMarker::new(
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
