use crate::{
	orchestrator::{
		self, IssueDispatchMode, TargetIssueRunContext,
		tests::{self, FakeTracker, TEST_SERVICE_ID, recovery_terminal_support},
	},
	state::StateStore,
	tracker,
	worktree::WorktreeManager,
};

#[test]
fn targeted_identifier_dispatch_rejects_different_status_visible_review_repair_lane() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let repair_issue = tests::sample_issue_with_sort_fields(
		"issue-repair",
		"PUB-201",
		"In Review",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let requested_issue = tests::sample_issue_with_sort_fields(
		"issue-requested",
		"PUB-202",
		"In Review",
		&[active_label.as_str()],
		Some(2),
		"2026-03-13T04:17:17.133Z",
	);
	let tracker = FakeTracker::new(vec![repair_issue.clone(), requested_issue.clone()]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&repair_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/185";

	state_store
		.upsert_worktree(
			config.service_id(),
			&repair_issue.id,
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

	let _path_guard = recovery_terminal_support::install_fake_conflicting_pr_gh_response(
		&temp_dir, &worktree, pr_url, &head_oid,
	);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");

	assert_eq!(snapshot.post_review_lanes.len(), 1);
	assert_eq!(snapshot.post_review_lanes[0].issue_identifier, repair_issue.identifier);
	assert_eq!(snapshot.post_review_lanes[0].classification, "needs_review_repair");
	assert_eq!(snapshot.post_review_lanes[0].reason, "pull_request_merge_conflict");

	let error = orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &requested_issue.identifier,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Normal,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect_err("targeted review repair inference should reject a different visible lane");
	let message = error.to_string();

	assert!(message.contains("targeted retained review repair mismatch"));
	assert!(message.contains(&requested_issue.identifier));
	assert!(message.contains(&repair_issue.identifier));
}
