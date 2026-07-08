use crate::orchestrator::tests::{
	self,
	runtime_failure::{
		FakeTracker, IssueDispatchMode, IssueRunPlan, PrepareIssueRunContext, StateStore,
		TEST_SERVICE_ID, WorktreeManager, WorktreeSpec, fs, orchestrator, tracker,
	},
};

#[test]
fn live_run_without_candidate_does_not_require_github_token_authority() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::with_refresh_snapshots_and_project(vec![], vec![vec![]], true);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("empty backlog should not require github token authority");

	assert!(summary.is_none());
}

#[test]
fn does_not_require_github_token_before_agent_execution() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let listed_issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![listed_issue.clone()],
		vec![vec![listed_issue.clone()]],
	);
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
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		},
		listed_issue.clone(),
	)
	.expect("candidate dispatch should prepare without github token authority")
	.expect("candidate issue should plan a run");

	assert_eq!(issue_run.issue.id, listed_issue.id);
	assert_eq!(issue_run.issue_state, "In Progress");
	assert!(
		state_store.lease_for_issue(&listed_issue.id).expect("lease lookup should work").is_some()
	);
	assert!(
		state_store
			.worktree_for_issue(&listed_issue.id)
			.expect("worktree lookup should work")
			.is_some()
	);
	assert_eq!(
		state_store
			.latest_run_attempt_for_issue(&listed_issue.id)
			.expect("run attempt lookup should work")
			.expect("starting attempt should record")
			.status(),
		"starting"
	);
}

#[test]
fn execute_issue_run_clears_lease_when_active_label_setup_fails() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let mut listed_issue = tests::sample_issue("Todo", &[]);
	let mut refreshed_issue = listed_issue.clone();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let worktree_path = config.worktree_root().join(&listed_issue.identifier);

	listed_issue.team.labels.retain(|label| label.name != active_label);
	refreshed_issue.team.labels.retain(|label| label.name != active_label);

	let tracker = FakeTracker::with_refresh_snapshots(
		vec![listed_issue.clone()],
		vec![vec![refreshed_issue]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = IssueRunPlan {
		issue: listed_issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: listed_issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: listed_issue.identifier.clone(),
			path: worktree_path.clone(),
			reused_existing: false,
		},
		retry_project_slug: listed_issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt(
			&issue_run.run_id,
			&listed_issue.id,
			issue_run.attempt_number,
			"starting",
		)
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &listed_issue.id, &issue_run.run_id, "In Progress")
		.expect("lease should record");

	let error = orchestrator::execute_issue_run(
		&tracker,
		&config,
		&workflow,
		&state_store,
		issue_run.clone(),
	)
	.expect_err("active-label setup failure should abort execution");

	assert!(error.to_string().contains("required label"));
	assert!(
		state_store.lease_for_issue(&listed_issue.id).expect("lease lookup should work").is_none(),
		"active-label setup failures should still release the lease"
	);
	assert_eq!(
		state_store
			.run_attempt(&issue_run.run_id)
			.expect("run attempt lookup should work")
			.expect("run attempt should exist")
			.status(),
		"failed",
		"active-label setup failures should mark the run failed before returning"
	);
}
