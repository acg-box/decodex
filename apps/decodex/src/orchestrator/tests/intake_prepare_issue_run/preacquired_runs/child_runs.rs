#[cfg(unix)] use std::os::fd::IntoRawFd;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, PreferredRunIdentity, TargetIssueRunContext,
		tests::{self, FakeTracker},
	},
	state::StateStore,
};

#[cfg(unix)]
#[test]
fn run_target_issue_once_skips_reconciliation_for_preacquired_child_runs() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![Vec::new(), Vec::new()]);
	let parent_store = StateStore::open_in_memory().expect("parent state store should open");
	let child_store = StateStore::open_in_memory().expect("child state store should open");

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
		.record_run_attempt("planned-run", &issue.id, 1, "running")
		.expect("adopted run attempt should record");

	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &child_store,
		issue_id: &issue.id,
		preferred_issue_state: Some("In Progress"),
		preferred_initial_issue_state: None,
		dry_run: false,
		lease_preacquired: true,
		preferred_issue_claim_fd: Some(child_issue_claim.into_raw_fd()),
		preferred_dispatch_slot_fd: Some(child_guard.into_raw_fd()),
		preferred_dispatch_slot_index: Some(child_slot_index),
		dispatch_mode: IssueDispatchMode::Normal,
		preferred_run_identity: Some(PreferredRunIdentity {
			run_id: "planned-run",
			attempt_number: 1,
		}),
		preferred_retry_budget_base: None,
	})
	.expect("targeted child run should not error before refresh lookup");

	assert!(summary.is_none(), "missing refreshed issue should stop before execution");
	assert_eq!(
		child_store
			.lease_for_issue(&issue.id)
			.expect("child lease lookup should succeed")
			.expect("preacquired child lease should remain adopted")
			.run_id(),
		"planned-run"
	);
	assert_eq!(
		child_store
			.run_attempt("planned-run")
			.expect("run lookup should succeed")
			.expect("planned attempt should remain recorded")
			.status(),
		"running"
	);
}
