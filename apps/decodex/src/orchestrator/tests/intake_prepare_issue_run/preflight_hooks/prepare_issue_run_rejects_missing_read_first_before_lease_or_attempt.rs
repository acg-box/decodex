use crate::{
	orchestrator::{
		self, IssueDispatchMode, PrepareIssueRunContext,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn prepare_issue_run_rejects_missing_read_first_before_lease_or_attempt() {
	let workflow_markdown = tests::sample_workflow_markdown(
		"pubfi",
		&["docs/guide/getting_started.md"],
		"Follow the repository policy.\n",
		1,
	);
	let (_temp_dir, base_config, workflow) =
		tests::temp_project_layout_with_workflow_markdown(&workflow_markdown);
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let error = orchestrator::prepare_issue_run(
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
		issue.clone(),
	)
	.expect_err("missing read_first file should reject dispatch");
	let message = format!("{error:#}");

	assert!(message.contains("context.read_first"));
	assert!(message.contains("docs/guide/getting_started.md"));
	assert!(
		message.contains(config.workflow_path().to_str().expect("workflow path should be utf-8"))
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"read_first preflight must reject before acquiring a lease"
	);
	assert!(
		state_store
			.list_run_attempts_for_issue(&issue.id)
			.expect("attempt lookup should succeed")
			.is_empty(),
		"read_first preflight must reject before recording an attempt"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_none(),
		"read_first preflight must reject before recording worktree ownership"
	);
}
