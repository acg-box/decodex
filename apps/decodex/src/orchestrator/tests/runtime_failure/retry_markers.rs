use crate::orchestrator::{
	RepoGateFailure,
	tests::{
		self,
		runtime_failure::{
			AppServerCapabilityPreflightFailure, IssueDispatchMode, IssueRunPlan, OffsetDateTime,
			RepoGateFailureKind, Report, StateStore, WorktreeSpec, fs, orchestrator, state,
		},
	},
};

#[test]
fn repo_gate_lock_contention_runtime_retry_writes_specific_retry_schedule_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path.clone(),
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
	let error = Report::new(RepoGateFailure::new(
		RepoGateFailureKind::GitLockContention,
		String::from(
			"Failed to inspect tracked-file cleanliness after repo gate verification in `/tmp/repo`: fatal: Unable to create '.git/index.lock': File exists.",
		),
	));

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	orchestrator::write_retry_schedule_marker_for_runtime_retry(&error, &workflow, &issue_run, 1)
		.expect("lock contention should write a specific retry marker");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("retry schedule should remain readable")
		.expect("retry marker should exist");

	assert_eq!(marker.retry_kind(), Some("git_lock_contention"));
	assert!(
		marker.retry_ready_at_unix_epoch().is_some_and(
			|retry_ready_at| retry_ready_at > OffsetDateTime::now_utc().unix_timestamp()
		)
	);
}

#[test]
fn app_server_preflight_timeout_runtime_retry_writes_failure_retry_schedule_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path.clone(),
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
	let error = Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
		"plugin/list",
		String::from("Timed out while waiting for app-server output."),
	));

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	orchestrator::write_retry_schedule_marker_for_runtime_retry(&error, &workflow, &issue_run, 1)
		.expect("preflight timeout should write a failure retry marker");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("retry schedule should remain readable")
		.expect("retry marker should exist");

	assert_eq!(marker.retry_kind(), Some("failure"));
	assert!(
		marker.retry_ready_at_unix_epoch().is_some_and(
			|retry_ready_at| retry_ready_at > OffsetDateTime::now_utc().unix_timestamp()
		)
	);
}

#[test]
fn retry_budget_current_failure_does_not_double_count_handed_off_base() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
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
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2-456"),
		retry_budget_base: 1,
	};

	state_store
		.record_lane_run_attempt(
			config.service_id(),
			"pub-101-attempt-1-123",
			&issue.id,
			1,
			"failed",
		)
		.expect("previous failed attempt should record");
	state_store
		.record_lane_run_attempt(
			config.service_id(),
			&issue_run.run_id,
			&issue.id,
			issue_run.attempt_number,
			"failed",
		)
		.expect("current failed attempt should record");

	assert_eq!(
		orchestrator::retry_budget_attempts_for_current_failure(
			&state_store,
			config.service_id(),
			&issue_run,
		)
		.expect("retry budget should compute"),
		2,
		"the daemon handoff base already includes the previous persisted failed attempt"
	);
}

#[test]
fn repo_gate_terminal_failures_preserve_specific_error_class_after_retry_exhaustion() {
	let error = Report::new(RepoGateFailure::new(
		RepoGateFailureKind::VerifyCommandFailed,
		String::from("Repo verify command `cargo make test` failed in `/tmp/repo`: test failed"),
	));
	let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
		false,
		&error,
		"clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
	);

	assert_eq!(error_class, "repo_gate_verify_failed");
	assert!(next_action.contains("repair the repo verification failure manually"));
}

#[test]
fn preserves_error_class_after_repo_gate_retry_exhaustion() {
	let error = Report::new(RepoGateFailure::new(
		RepoGateFailureKind::GitLockContention,
		String::from(
			"Failed to inspect tracked-file cleanliness after repo gate verification in `/tmp/repo`: fatal: Unable to create '.git/index.lock': File exists.",
		),
	));
	let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
		false,
		&error,
		"clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
	);

	assert_eq!(error_class, "repo_gate_git_lock_contention");
	assert!(next_action.contains("active or stale `.git/index.lock` holder"));
}
