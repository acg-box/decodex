use crate::orchestrator::{
	tests,
	tests::runtime_failure::{
		AppServerCapabilityPreflightFailure, AppServerHomePreflightFailure,
		AppServerPhaseGoalFailure, AppServerTransportFailure, AppServerTurnFailure,
		AppServerZeroEvidenceStartFailure, BTreeMap, CodexAccountAuthFailure, FakeTracker,
		IssueDispatchMode, IssueRunPlan, PhaseGoalKind, Report, StateStore, TestEnvVarGuard,
		WorktreeSpec, fs, orchestrator,
	},
};

#[test]
fn app_server_terminal_failures_preserve_specific_error_classes() {
	let cases = [
		(
			Report::new(AppServerCapabilityPreflightFailure::blocked_for_test(
				"model",
				"configured model was not present in model/list.",
			)),
			"app_server_runtime_preflight_failed",
			"repair the local Codex runtime configuration",
		),
		(
			Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
				"plugin/list",
				String::from("Timed out while waiting for app-server output."),
			)),
			"app_server_plugin_list_timeout",
			"app_server_preflight_failed evidence for the `plugin/list` timeout",
		),
		(
			Report::new(AppServerHomePreflightFailure::resolution_failed(String::from(
				"app_server_preflight_failed: HOME is not set, so Decodex cannot resolve the shared Codex home for app-server dispatch.",
			))),
			"app_server_codex_home_preflight_failed",
			"inspect the local Decodex and Codex home sharing",
		),
		(
			Report::new(AppServerHomePreflightFailure::initialize_mismatch(
				String::from("/tmp/per-account-codex-home"),
				String::from("/Users/test/.codex"),
			)),
			"app_server_codex_home_mismatch",
			"restart `decodex serve`",
		),
		(
			Report::new(AppServerTransportFailure::new(String::from(
				"App-server stdout disconnected unexpectedly.",
			))),
			"app_server_transport_disconnected",
			"inspect the local app-server stderr tail",
		),
		(
			Report::new(AppServerZeroEvidenceStartFailure::new(
				String::from("PUB-101"),
				String::from("pub-101-attempt-1-123"),
			)),
			"app_server_zero_evidence_start_failed",
			"verify `decodex probe stdio://`",
		),
		(
			Report::new(CodexAccountAuthFailure::new(
				Some(String::from("...123456")),
				Some(String::from("bad@example.com")),
				"Codex account `bad@example.com` token refresh failed with HTTP 401 Unauthorized.",
			)),
			"codex_account_auth_failed",
			"re-login or remove Decodex Codex account",
		),
		(
			Report::new(AppServerPhaseGoalFailure::missing_terminal_path_for_test(
				PhaseGoalKind::HandoffEvidence,
			)),
			"phase_goal_terminal_path_missing",
			"finish validation/review/handoff",
		),
		(
			Report::new(AppServerTurnFailure::new(
				"thread-1",
				Some(String::from("turn-1")),
				"failed",
				"You've hit your usage limit.",
				Some(String::from("usageLimitExceeded")),
			)),
			"app_server_usage_limit_exceeded",
			"inspect Codex account usage",
		),
	];

	for (error, expected_class, expected_action) in cases {
		let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
			false,
			&error,
			"clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
		);

		assert_eq!(error_class, expected_class);
		assert!(next_action.contains(expected_action));
		assert!(next_action.contains("clear label `decodex:needs-attention`"));
	}
}

#[test]
fn app_server_preflight_terminal_action_surfaces_first_scan_error() {
	let mut details = BTreeMap::new();

	details.insert(
		String::from("first_error_path"),
		String::from("/tmp/plugins/build-web-data-visualization/skills/chart/SKILL.md"),
	);
	details.insert(
		String::from("first_error"),
		String::from("name: exceeds maximum length of 64 characters"),
	);

	let error = Report::new(AppServerCapabilityPreflightFailure::blocked_for_test_with_details(
		"skills",
		"skills/list returned no enabled skills.",
		details,
	));
	let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
		false,
		&error,
		"clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
	);

	assert_eq!(error_class, "app_server_runtime_preflight_failed");
	assert!(next_action.contains("first_error_path=/tmp/plugins/build-web-data-visualization"));
	assert!(next_action.contains("first_error=name: exceeds maximum length of 64 characters"));
}

#[test]
fn zero_evidence_app_server_start_failure_is_promoted_records_private_evidence_and_retries() {
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

#[test]
fn exhausted_zero_evidence_start_retry_budget_requires_attention_with_typed_class() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
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
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3-123"),
		retry_budget_base: 2,
	};
	let error = Report::new(AppServerZeroEvidenceStartFailure::new(
		issue.identifier.clone(),
		issue_run.run_id.clone(),
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("exhausted zero-evidence failure should require attention");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and needs attention")
			&& comment.contains("app_server_zero_evidence_start_failed")
			&& comment.contains("verify `decodex probe stdio://`")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and will retry")),
		"exhausted zero-evidence failure should not keep retrying"
	);
}

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

#[test]
fn retryable_turn_failure_does_not_promote_to_zero_evidence_attention() {
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
		Report::new(AppServerTurnFailure::new(
			"thread-1",
			Some(String::from("turn-1")),
			"failed",
			"You've hit your usage limit.",
			Some(String::from("usageLimitExceeded")),
		)),
	);

	assert!(
		error.downcast_ref::<AppServerTurnFailure>().is_some(),
		"structured turn failures should stay retryable instead of becoming zero-evidence terminal attention"
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
