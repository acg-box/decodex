use crate::{
	orchestrator::{
		self, IssueDispatchMode, PreferredRunIdentity, PrepareIssueRunContext,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn prepare_issue_run_rejects_stale_preferred_identity_after_attempt_advance() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	state_store
		.record_lane_run_attempt(config.service_id(), "other-run", &issue.id, 1, "succeeded")
		.expect("existing run attempt should record");

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
	.expect("stale targeted issue preparation should not error");

	assert!(issue_run.is_none(), "stale preferred identity should be rejected");
	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
}
