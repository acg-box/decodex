use crate::orchestrator::{
	tests,
	tests::runtime_failure::{
		AppServerTransportFailure, IssueDispatchMode, IssueRunPlan, Report, StateStore,
		WorktreeSpec, fs, orchestrator,
	},
};

#[test]
fn retryable_startup_transport_failure_does_not_promote_to_zero_evidence_attention() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	let error = orchestrator::promote_zero_evidence_app_server_start_failure(
		&config,
		&state_store,
		&issue_run,
		Report::new(AppServerTransportFailure::with_phase(
			String::from("App-server stdout disconnected unexpectedly."),
			"thread/start",
			true,
		)),
	);

	assert!(
		error.downcast_ref::<AppServerTransportFailure>().is_some(),
		"startup transport failures should stay retryable instead of becoming zero-evidence terminal attention"
	);
	assert!(error.downcast_ref::<orchestrator::AppServerZeroEvidenceStartFailure>().is_none());

	let events = state_store
		.list_private_execution_events(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)
		.expect("private evidence should list");

	assert!(events.is_empty());
}
