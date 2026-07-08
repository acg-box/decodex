use std::fs;

use crate::{
	orchestrator::{
		self, RecoverableWorktreeSkipCache,
		tests::{self, FakeTracker, TEST_SERVICE_ID, recovery_terminal_support},
	},
	state::{self, StateStore},
	tracker,
	worktree::WorktreeManager,
};

#[test]
fn idle_daemon_recovery_reconstructs_completed_closeout_worktree_mapping() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_DAEMON_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
	let issue = recovery_terminal_support::sample_active_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/178";

	tests::seed_review_lifecycle_handoff_fixture_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);
	orchestrator::recover_and_reconcile_idle_daemon_state(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&worktree_manager,
		None,
	)
	.expect("idle daemon recovery should succeed");

	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"idle daemon recovery should reconstruct retained closeout worktree mappings from disk"
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"blocked retained closeout recovery should not invent a live lease without fresh activity"
	);
}

#[test]
fn live_run_clears_claimed_lease_when_refresh_fails_after_worktree_prepare() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let listed_issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_error(vec![listed_issue.clone()], "transient refresh failure");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let error = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect_err("run once should propagate refresh failure");

	assert!(
		error.to_string().contains("transient refresh failure"),
		"error should surface the refresh failure"
	);
	assert!(
		state_store.lease_for_issue(&listed_issue.id).expect("lease lookup should work").is_none()
	);
}

#[test]
fn live_run_skips_issue_that_becomes_ineligible_after_worktree_prepare() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let listed_issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![listed_issue.clone()],
		vec![vec![], vec![listed_issue.clone()], vec![tests::sample_issue("In Progress", &[])]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("run once should succeed");

	assert!(summary.is_none());
	assert!(
		state_store.lease_for_issue(&listed_issue.id).expect("lease lookup should work").is_none()
	);
	assert!(
		state_store
			.worktree_for_issue(&listed_issue.id)
			.expect("worktree lookup should work")
			.is_some()
	);
	assert!(tracker.comments.borrow().is_empty());
}

#[test]
fn recovery_skip_cache_suppresses_repeated_unowned_worktree_lookup() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(Vec::new()).with_identifier_lookup_issues(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let mut skip_cache = RecoverableWorktreeSkipCache::default();

	fs::create_dir_all(&worktree_path).expect("stale worktree directory should exist");

	let first = orchestrator::recover_runtime_state_with_skip_cache(
		&tracker,
		&config,
		&workflow,
		&state_store,
		Some(&mut skip_cache),
	)
	.expect("first recovery probe should succeed");
	let second = orchestrator::recover_runtime_state_with_skip_cache(
		&tracker,
		&config,
		&workflow,
		&state_store,
		Some(&mut skip_cache),
	)
	.expect("cached recovery probe should succeed");
	let identifier_queries = tracker.identifier_queries.borrow();

	assert!(first.recoverable_issues.is_empty());
	assert!(second.recoverable_issues.is_empty());
	assert_eq!(identifier_queries.len(), 1);
	assert_eq!(identifier_queries[0], issue.identifier);
	assert!(
		tracker.refresh_queries.borrow().is_empty(),
		"empty known issue sets should not call tracker refresh"
	);
	assert!(
		tracker.label_queries.borrow().is_empty(),
		"complete issue labels should not need server confirmation"
	);
}

#[test]
fn run_project_once_ignores_fresh_marker_for_exited_process() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should exist");
	let exited_process_id = u32::MAX;

	state::write_run_activity_marker_for_process(&worktree.path, "run-1", 1, exited_process_id)
		.expect("activity marker should write");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovery should succeed")
		.expect("dead process marker should not block retry planning");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"dead marker recovery should not reconstruct a live lease"
	);
}

#[test]
fn run_project_once_recovers_worktree_when_identifier_lookup_labels_are_truncated() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let listed_issue = recovery_terminal_support::sample_active_issue("In Progress");
	let mut identifier_lookup_issue = listed_issue.clone();

	identifier_lookup_issue.labels_complete = false;

	identifier_lookup_issue.labels.retain(|label| label.name != active_label);

	let tracker = FakeTracker::with_refresh_snapshots(
		vec![listed_issue.clone()],
		vec![vec![listed_issue.clone()]],
	)
	.with_identifier_lookup_issues(vec![identifier_lookup_issue]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let expected_worktree = worktree_manager
		.ensure_worktree(&listed_issue.identifier, false)
		.expect("recovered worktree should be created")
		.path;
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovery should succeed")
		.expect("ambiguous label pagination should still recover the owned retained lane");

	assert_eq!(summary.issue_id, listed_issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert_eq!(summary.worktree_path, expected_worktree);
}

#[test]
fn run_project_once_skips_recovered_worktree_without_service_active_label() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("foreign retained worktree should exist");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovery should succeed");

	assert!(
		summary.is_none(),
		"recovery should skip retained worktrees that are not explicitly owned by this service"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_none(),
		"foreign retained worktrees should not be reconstructed into local service state"
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"foreign retained worktrees should not rebuild service leases"
	);
}
