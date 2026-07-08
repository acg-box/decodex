use crate::{
	orchestrator::{
		self, IssueDispatchMode, PreferredRunIdentity, PrepareIssueRunContext,
		tests::{self, FakeTracker, TEST_SERVICE_ID},
	},
	state::{self, StateStore},
	tracker,
	worktree::WorktreeManager,
};

#[test]
fn prepare_issue_run_starts_fresh_retry_budget_for_normal_queue_intake() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");

	state::write_run_retry_budget_attempt_count(&worktree.path, "older-run", 4, 2)
		.expect("retry budget marker should write");

	let issue_run = orchestrator::prepare_issue_run(
		PrepareIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			worktree_manager: &worktree_manager,
			dry_run: false,
			lease_preacquired: false,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		},
		issue,
	)
	.expect("issue preparation should succeed")
	.expect("startable issue should prepare");

	assert_eq!(
		issue_run.retry_budget_base, 0,
		"normal queue intake starts a new automatic retry episode instead of inheriting old marker attempts"
	);
}

#[test]
fn prepare_issue_run_uses_persisted_retry_budget_marker_for_recovered_retry() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");

	state::write_run_retry_budget_attempt_count(&worktree.path, "older-run", 4, 2)
		.expect("retry budget marker should write");

	let issue_run = orchestrator::prepare_issue_run(
		PrepareIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			worktree_manager: &worktree_manager,
			dry_run: false,
			lease_preacquired: false,
			dispatch_mode: IssueDispatchMode::Retry,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		},
		issue,
	)
	.expect("issue preparation should succeed")
	.expect("startable issue should prepare");

	assert_eq!(
		issue_run.retry_budget_base, 2,
		"recovered retry dispatch should preserve retry budget from the retained worktree marker"
	);
}

#[test]
fn keeps_retry_budget_when_preferred_base_stale() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");

	state::write_run_retry_budget_attempt_count(&worktree.path, "older-run", 4, 2)
		.expect("retry budget marker should write");

	let issue_run = orchestrator::prepare_issue_run(
		PrepareIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			worktree_manager: &worktree_manager,
			dry_run: false,
			lease_preacquired: false,
			dispatch_mode: IssueDispatchMode::Retry,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_run_identity: None,
			preferred_retry_budget_base: Some(0),
		},
		issue,
	)
	.expect("issue preparation should succeed")
	.expect("recovered retry issue should prepare");

	assert_eq!(
		issue_run.retry_budget_base, 2,
		"preferred retry-budget base should not hide retained retry episode state"
	);
}

#[test]
fn prepare_issue_run_honors_preferred_identity_when_attempt_is_current() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let issue_run = orchestrator::prepare_issue_run(
		PrepareIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			worktree_manager: &worktree_manager,
			dry_run: false,
			lease_preacquired: false,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_run_identity: Some(PreferredRunIdentity {
				run_id: "planned-run",
				attempt_number: 1,
			}),
			preferred_retry_budget_base: None,
		},
		issue.clone(),
	)
	.expect("issue preparation should succeed")
	.expect("targeted issue should prepare");

	assert_eq!(issue_run.run_id, "planned-run");
	assert_eq!(issue_run.attempt_number, 1);
	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("lease should exist")
			.run_id(),
		"planned-run"
	);
}
