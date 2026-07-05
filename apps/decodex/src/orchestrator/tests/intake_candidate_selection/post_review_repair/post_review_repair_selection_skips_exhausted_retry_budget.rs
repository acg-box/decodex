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
fn post_review_repair_selection_skips_exhausted_retry_budget() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let repair_issue = support::candidate_selection_service_owned_issue("In Review");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![repair_issue.clone()],
		vec![vec![repair_issue.clone()]],
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&repair_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/174";

	state_store
		.upsert_worktree(
			config.service_id(),
			&repair_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	for attempt in 1..=3 {
		state_store
			.record_run_attempt(
				&format!("run-review-repair-{attempt}"),
				&repair_issue.id,
				attempt,
				"failed",
			)
			.expect("failed repair attempt should record");
	}

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&repair_issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let inspector = FakePullRequestReviewStateInspector::new(vec![Ok(
		tests::sample_pull_request_review_state(
			pr_url,
			&worktree.branch_name,
			&head_oid,
			Some("CHANGES_REQUESTED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		),
	)]);
	let selected = orchestrator::select_post_review_repair_issue_candidate_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&[],
		&inspector,
	)
	.expect("post-review repair selection should succeed");

	assert!(selected.is_none(), "exhausted repair lanes should not be redispatched");
}
