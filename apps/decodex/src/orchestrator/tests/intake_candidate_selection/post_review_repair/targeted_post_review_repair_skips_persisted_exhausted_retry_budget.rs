use crate::{
	orchestrator::{
		self, IssueDispatchMode, TargetIssueRunContext,
		tests::{self, FakeTracker, intake_candidate_selection::support},
	},
	state::{self, StateStore},
	worktree::WorktreeManager,
};

#[test]
fn targeted_post_review_repair_skips_persisted_exhausted_retry_budget() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let repair_issue = support::candidate_selection_service_owned_issue("In Review");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![repair_issue.clone()],
		vec![vec![repair_issue.clone()], vec![repair_issue.clone()]],
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&repair_issue.identifier, false)
		.expect("worktree should exist");

	state_store
		.upsert_worktree(
			config.service_id(),
			&repair_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_retry_budget_attempt_count(&worktree.path, "older-run", 3, 3)
		.expect("retry budget marker should write");

	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &repair_issue.id,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect("targeted review-repair planning should succeed");

	assert!(summary.is_none(), "persisted exhausted budget should block direct repair dispatch");
}
