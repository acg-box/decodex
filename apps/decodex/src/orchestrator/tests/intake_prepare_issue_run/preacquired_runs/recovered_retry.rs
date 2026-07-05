#[cfg(unix)] use std::os::fd::IntoRawFd;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, PreferredRunIdentity, PrepareIssueRunContext,
		tests::{self, FakeTracker, TEST_SERVICE_ID},
	},
	state::{PreacquiredLeaseGuards, StateStore},
	tracker,
	worktree::WorktreeManager,
};

#[cfg(unix)]
#[test]
fn prepare_issue_run_allows_preacquired_recovered_retry_attempt() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let parent_store = StateStore::open_in_memory().expect("parent state store should open");
	let child_store = StateStore::open_in_memory().expect("child state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	parent_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("parent dispatch-slot root should configure");
	child_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("child dispatch-slot root should configure");

	assert!(
		parent_store
			.try_acquire_lease(config.service_id(), &issue.id, "planned-run", "In Progress")
			.expect("parent should acquire the shared dispatch slot")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child(&issue.id)
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child(&issue.id)
		.expect("child should inherit the shared dispatch-slot fd");

	child_store
		.adopt_preacquired_lease(
			config.service_id(),
			&issue.id,
			"planned-run",
			"In Progress",
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");
	child_store
		.record_run_attempt("planned-run", &issue.id, 1, "running")
		.expect("recovered attempt should record before targeted execution");

	let issue_run = orchestrator::prepare_issue_run(
		PrepareIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &child_store,
			worktree_manager: &worktree_manager,
			dry_run: false,
			lease_preacquired: true,
			dispatch_mode: IssueDispatchMode::Retry,
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
	.expect("recovered retry preparation should succeed")
	.expect("planned retry attempt should still execute");

	assert_eq!(issue_run.run_id, "planned-run");
	assert_eq!(issue_run.attempt_number, 1);
	assert_eq!(
		child_store
			.lease_for_issue(&issue.id)
			.expect("child lease lookup should succeed")
			.expect("child should retain the adopted local lease")
			.run_id(),
		"planned-run"
	);
}
