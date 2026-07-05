use std::fs;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, PrepareIssueRunContext, TERMINAL_GUARD_MARKER_FILE,
		tests::{
			FakeTracker, {self},
		},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn prepare_issue_run_clears_terminal_guard_marker_when_new_attempt_starts() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(vec![], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("worktree should exist before retry guard clearing");
	let marker_path = worktree.path.join(TERMINAL_GUARD_MARKER_FILE);

	fs::write(&marker_path, "stale terminal guard\n").expect("terminal guard marker should write");

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
	.expect("startable issue should produce a run plan");

	assert_eq!(issue_run.worktree.path, worktree.path);
	assert!(
		!marker_path.exists(),
		"starting a new attempt should clear stale terminal-guard markers"
	);
}
