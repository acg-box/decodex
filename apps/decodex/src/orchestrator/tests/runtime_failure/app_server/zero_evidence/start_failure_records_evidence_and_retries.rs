use crate::orchestrator::{
	tests,
	tests::runtime_failure::{
		FakeTracker, IssueDispatchMode, IssueRunPlan, Report, StateStore, TestEnvVarGuard,
		WorktreeSpec, fs, orchestrator,
	},
};

#[test]
fn start_failure_records_evidence_and_retries() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let _env_guard =
		TestEnvVarGuard::set("DECODEX_TEST_ZERO_EVIDENCE_SECRET_TOKEN", "synthetic-secret-token");
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
		Report::msg("synthetic startup failure: synthetic-secret-token"),
	);

	assert!(
		error.downcast_ref::<orchestrator::AppServerZeroEvidenceStartFailure>().is_some(),
		"generic no-evidence startup errors should become typed app-server start failures"
	);

	let events = state_store
		.list_private_execution_events(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)
		.expect("private evidence should list");

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_type(), "app_server_zero_evidence_start_failure");
	assert_eq!(events[0].payload()["error_class"], "app_server_zero_evidence_start_failed");
	assert_eq!(events[0].payload()["protocol_event_count"], 0);
	assert_eq!(events[0].payload()["thread_recorded"], false);
	assert_eq!(
		events[0].payload()["source_error_summary"],
		"synthetic startup failure: <redacted env:DECODEX_TEST_ZERO_EVIDENCE_SECRET_TOKEN>"
	);
	assert_eq!(
		events[0].payload()["source_error_chain"][0],
		"synthetic startup failure: <redacted env:DECODEX_TEST_ZERO_EVIDENCE_SECRET_TOKEN>"
	);
	assert!(
		!events[0].payload().to_string().contains("synthetic-secret-token"),
		"private diagnostic payload must redact known secret env values"
	);

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("retryable zero-evidence failure handling should succeed");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("app_server_zero_evidence_start_failed")
			&& comment.contains("restart the app-server and retry automatically")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention")),
		"zero-evidence startup failure should not request operator attention before retry budget exhaustion"
	);
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("retryable_execution_failure")),
		"zero-evidence startup failure must preserve its typed retry class"
	);
}
