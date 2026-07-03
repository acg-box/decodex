use crate::{
	orchestrator::{
		self, DaemonTickRuntimeContext, RetryQueue, ReviewLevel, StateStore, tests,
		tests::{
			FakePullRequestReviewStateInspector, FakeTracker, TEST_SERVICE_ID,
			retry_scheduling::support::{
				self, PUB_704_RETAINED_HEAD_SUBJECT, PUB_704_RETAINED_LANDED_SUBJECT,
			},
		},
	},
	worktree::WorktreeManager,
};

#[test]
fn daemon_tick_reconciles_ready_retained_review_lane_before_dry_run_planning() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&base_config,
			"PATH",
			&gh_command_path,
		),
		ReviewLevel::Standard,
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let active_issue = support::sample_service_owned_issue_with_project_slug_and_sort_fields(
		"issue-active",
		"PUB-200",
		TEST_SERVICE_ID,
		"In Progress",
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let retained_issue = support::sample_service_owned_issue_with_project_slug_and_sort_fields(
		"issue-retained",
		"PUB-704",
		TEST_SERVICE_ID,
		"In Review",
		Some(2),
		"2026-03-13T04:18:17.133Z",
	);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![active_issue.clone(), retained_issue.clone()],
		vec![vec![active_issue.clone()], vec![retained_issue.clone()]],
	);
	let retained_worktree = worktree_manager
		.ensure_worktree(&retained_issue.identifier, false)
		.expect("retained worktree should exist");
	let pr_url = "https://github.com/hack-ink/decodex/pull/704";
	let head_oid = tests::commit_worktree_change(
		&retained_worktree.path,
		"retained.txt",
		"ready\n",
		PUB_704_RETAINED_HEAD_SUBJECT,
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&retained_issue.id,
			&retained_worktree.branch_name,
			&retained_worktree.path.display().to_string(),
		)
		.expect("retained worktree should record");
	state_store
		.record_run_attempt("leased-run", &active_issue.id, 1, "running")
		.expect("current lane should record");

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&retained_issue.id,
		&tests::sample_review_handoff_marker(&retained_worktree.branch_name, pr_url, &head_oid),
	);

	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		&retained_worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);
	let mut active_children = vec![support::spawn_sleeping_daemon_child(&active_issue, &workflow)];
	let mut retry_queue = RetryQueue::default();
	let result = orchestrator::run_daemon_tick_with_review_state_inspector(
		&tests::service_config_path(config.repo_root()),
		&state_store,
		&mut active_children,
		&mut retry_queue,
		DaemonTickRuntimeContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			worktree_manager: &worktree_manager,
			review_state_inspector: &FakePullRequestReviewStateInspector::new(vec![Ok(
				review_state,
			)]),
			recoverable_worktree_skip_cache: None,
		},
	);

	support::stop_daemon_children(&mut active_children);

	result.expect("daemon tick should reconcile retained review lanes");

	let marker = tests::persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&retained_worktree.path,
	);

	assert_eq!(marker.phase(), "waiting_for_merge");

	support::assert_fake_admin_merge_invocation_present(
		&invocation_log_path,
		&head_oid,
		PUB_704_RETAINED_LANDED_SUBJECT,
		pr_url,
	);
}

#[test]
fn daemon_tick_clears_terminal_mapping_without_worktree_before_retained_land() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&base_config,
			"PATH",
			&gh_command_path,
		),
		ReviewLevel::Standard,
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let mut terminal_issue = support::sample_service_owned_issue_with_project_slug_and_sort_fields(
		"issue-terminal",
		"PUB-703",
		TEST_SERVICE_ID,
		"Done",
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);

	terminal_issue.labels.clear();
	terminal_issue.team.labels.clear();

	let retained_issue = support::sample_service_owned_issue_with_project_slug_and_sort_fields(
		"issue-retained",
		"PUB-704",
		TEST_SERVICE_ID,
		"In Review",
		Some(2),
		"2026-03-13T04:18:17.133Z",
	);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![terminal_issue.clone(), retained_issue.clone()],
		vec![
			vec![terminal_issue.clone(), retained_issue.clone()],
			vec![terminal_issue.clone(), retained_issue.clone()],
			vec![retained_issue.clone()],
		],
	);
	let retained_worktree = worktree_manager
		.ensure_worktree(&retained_issue.identifier, false)
		.expect("retained worktree should exist");
	let pr_url = "https://github.com/hack-ink/decodex/pull/704";
	let head_oid = tests::commit_worktree_change(
		&retained_worktree.path,
		"retained.txt",
		"ready\n",
		PUB_704_RETAINED_HEAD_SUBJECT,
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&terminal_issue.id,
			"x/pubfi-pub-703",
			&config.worktree_root().join("PUB-703").display().to_string(),
		)
		.expect("terminal stale worktree should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&retained_issue.id,
			&retained_worktree.branch_name,
			&retained_worktree.path.display().to_string(),
		)
		.expect("retained worktree should record");

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&retained_issue.id,
		&tests::sample_review_handoff_marker(&retained_worktree.branch_name, pr_url, &head_oid),
	);

	let mut active_children = Vec::new();
	let mut retry_queue = RetryQueue::default();

	orchestrator::run_daemon_tick_with_review_state_inspector(
		&tests::service_config_path(config.repo_root()),
		&state_store,
		&mut active_children,
		&mut retry_queue,
		DaemonTickRuntimeContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			worktree_manager: &worktree_manager,
			review_state_inspector: &FakePullRequestReviewStateInspector::new(vec![Ok(
				support::sample_approved_clean_review_state(
					pr_url,
					&retained_worktree.branch_name,
					&head_oid,
				),
			)]),
			recoverable_worktree_skip_cache: None,
		},
	)
	.expect("daemon tick should not fail on stale terminal worktree state");

	assert!(
		state_store
			.worktree_for_issue(&terminal_issue.id)
			.expect("terminal worktree lookup should succeed")
			.is_none(),
		"terminal mapping without a local worktree should be cleared"
	);

	support::assert_fake_admin_merge_invocation_present(
		&invocation_log_path,
		&head_oid,
		PUB_704_RETAINED_LANDED_SUBJECT,
		pr_url,
	);
}
