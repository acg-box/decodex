use time::OffsetDateTime;

use crate::{
	orchestrator::{
		self, ChildRunRef, CurrentChildRunContext, IssueDispatchMode, StateStore,
		tests::{self, FakeTracker, recovery_reconciliation::support},
	},
	state,
	worktree::WorktreeManager,
};

#[test]
fn run_lease_reconciliation_keeps_completed_closeout_lane_with_fresh_activity() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_ACTIVE_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
	let issue = support::reconciliation_sample_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-closeout-active";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/180";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store.upsert_lease("pubfi", &issue.id, run_id, "Done").expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);
	state::write_run_activity_marker(&worktree.path, run_id, 1)
		.expect("fresh activity marker should write");

	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		OffsetDateTime::now_utc().unix_timestamp(),
	)
	.expect("run lease inspection should succeed");

	assert!(
		actions.is_empty(),
		"completed retained closeout lanes with fresh activity must not be reconciled as terminal or not-dispatchable"
	);
}

#[test]
fn active_daemon_child_reconciliation_keeps_completed_closeout_lane_with_fresh_activity() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_ACTIVE_DAEMON_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
	let issue = support::reconciliation_sample_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-daemon-closeout-active";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/181";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store.upsert_lease("pubfi", &issue.id, run_id, "Done").expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);
	state::write_run_activity_marker(&worktree.path, run_id, 1)
		.expect("fresh activity marker should write");

	let actions = orchestrator::inspect_current_daemon_child_reconciliation(
		&tracker,
		&config,
		&workflow,
		&state_store,
		CurrentChildRunContext {
			child: ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
			workflow: &workflow,
			dispatch_mode: IssueDispatchMode::Closeout,
		},
	)
	.expect("current daemon-child inspection should succeed");

	assert!(
		actions.is_empty(),
		"completed retained closeout daemon children with fresh activity must not be reconciled as terminal or not-dispatchable"
	);
}

#[test]
fn current_daemon_child_reconciliation_keeps_closeout_child_after_tracker_completion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Done", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-closeout-completed";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "Done")
		.expect("lease should record");

	let actions = orchestrator::inspect_current_daemon_child_reconciliation(
		&tracker,
		&config,
		&workflow,
		&state_store,
		CurrentChildRunContext {
			child: ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
			workflow: &workflow,
			dispatch_mode: IssueDispatchMode::Closeout,
		},
	)
	.expect("current closeout daemon-child inspection should succeed");

	assert!(
		actions.is_empty(),
		"closeout children may legitimately observe a completed tracker issue while they finish local cleanup"
	);
}
