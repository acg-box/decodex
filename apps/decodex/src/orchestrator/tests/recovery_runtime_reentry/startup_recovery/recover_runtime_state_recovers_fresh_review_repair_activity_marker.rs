use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, recovery_terminal_support},
	},
	state::{self, StateStore},
	worktree::WorktreeManager,
};

#[test]
fn recover_runtime_state_recovers_fresh_review_repair_activity_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Review");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("review-repair worktree should exist");

	state::write_run_activity_marker(&worktree.path, "run-review-repair", 1)
		.expect("fresh activity marker should write");

	let recovered = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should succeed");

	assert!(
		recovered.recoverable_issues.is_empty(),
		"fresh review-repair activity should rebuild the lease instead of requeueing the lane"
	);

	let lease = state_store
		.lease_for_issue(&issue.id)
		.expect("lease lookup should succeed")
		.expect("fresh review-repair lane should rebuild its lease");

	assert_eq!(lease.run_id(), "run-review-repair");
	assert_eq!(lease.issue_state(), workflow.frontmatter().tracker().success_state());
}
