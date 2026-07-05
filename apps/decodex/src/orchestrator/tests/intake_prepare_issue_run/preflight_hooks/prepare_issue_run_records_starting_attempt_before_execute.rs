use crate::{
	orchestrator::{
		self, IssueDispatchMode, PrepareIssueRunContext,
		tests::{self, FakeTracker, TEST_SERVICE_ID},
	},
	state::StateStore,
	tracker::{self, records},
	worktree::WorktreeManager,
};

#[test]
fn prepare_issue_run_records_starting_attempt_before_execute() {
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
		issue.clone(),
	)
	.expect("issue preparation should succeed")
	.expect("active retry issue should prepare");

	assert_eq!(
		state_store
			.run_attempt(&issue_run.run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"starting"
	);
	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("lease should exist")
			.run_id(),
		issue_run.run_id
	);

	let event_types = tracker
		.comments
		.borrow()
		.iter()
		.filter_map(|comment| records::parse_linear_execution_event_record(comment))
		.map(|record| record.event_type)
		.collect::<Vec<_>>();

	assert_eq!(event_types, vec![String::from("run_started")]);
}
