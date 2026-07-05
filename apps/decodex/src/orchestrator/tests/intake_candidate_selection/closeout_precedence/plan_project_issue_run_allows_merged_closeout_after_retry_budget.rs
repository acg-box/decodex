use crate::{
	orchestrator::{
		self, IssueDispatchMode, TargetIssueRunContext,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker,
			intake_candidate_selection::support,
		},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn plan_project_issue_run_allows_merged_closeout_after_retry_budget() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let closeout_issue = support::candidate_selection_service_owned_issue("In Review");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![closeout_issue.clone()],
		vec![
			vec![closeout_issue.clone()],
			vec![closeout_issue.clone()],
			vec![closeout_issue.clone()],
		],
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
	tests::seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_orchestration_marker(
			&worktree.branch_name,
			pr_url,
			&head_oid,
			"waiting_for_merge",
			1,
		),
	);

	for attempt in 1..=3 {
		state_store
			.record_run_attempt(
				&format!("run-closeout-{attempt}"),
				&closeout_issue.id,
				attempt,
				"failed",
			)
			.expect("failed attempt should record");
	}

	let _path_guard = support::install_merged_pr_response(&temp_dir, &worktree, pr_url, &head_oid);
	let mut merged_review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	merged_review_state.state = String::from("MERGED");

	let inspector = FakePullRequestReviewStateInspector::new(vec![
		Ok(merged_review_state.clone()),
		Ok(merged_review_state),
	]);
	let selected = orchestrator::select_post_review_issue_candidate_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&[],
		&inspector,
	)
	.expect("post-review closeout selection should succeed")
	.expect("closeout lane should be selected");

	assert_eq!(selected.issue.identifier, closeout_issue.identifier);
	assert_eq!(selected.dispatch_mode, orchestrator::IssueDispatchMode::Closeout);

	let tracker = FakeTracker::with_refresh_snapshots(
		vec![closeout_issue.clone()],
		vec![vec![closeout_issue.clone()], vec![closeout_issue.clone()]],
	);
	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &closeout_issue.id,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect("targeted closeout planning should succeed")
	.expect("closeout issue run should plan");

	assert_eq!(summary.issue_identifier, closeout_issue.identifier);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Closeout);
}
