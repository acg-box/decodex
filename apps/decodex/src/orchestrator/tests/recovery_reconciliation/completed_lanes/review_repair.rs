use crate::{
	orchestrator::{
		self, ChildRunRef, CurrentChildRunContext, IssueDispatchMode, StateStore,
		tests::{self, FakeTracker, recovery_reconciliation::support},
	},
	worktree::WorktreeManager,
};

#[test]
fn current_daemon_child_reconciliation_keeps_review_repair_lane_in_review() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::reconciliation_sample_service_owned_issue("In Review");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-review-repair-current";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained review-repair worktree should exist");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "In Review")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let actions = orchestrator::inspect_current_daemon_child_reconciliation(
		&tracker,
		&config,
		&workflow,
		&state_store,
		CurrentChildRunContext {
			child: ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
			workflow: &workflow,
			dispatch_mode: IssueDispatchMode::ReviewRepair,
		},
	)
	.expect("current review-repair daemon-child inspection should succeed");

	assert!(
		actions.is_empty(),
		"review-repair lanes in In Review must stay current instead of being interrupted as not-dispatchable"
	);
}
