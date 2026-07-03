use std::{fs, time::Duration};

use color_eyre::Report;

use crate::{
	agent::{
		AppServerCapabilityPreflightFailure, AppServerDynamicToolFailure,
		AppServerPhaseGoalFailure, AppServerTransportFailure, AppServerTurnFailure,
	},
	orchestrator::{
		self, AppServerZeroEvidenceStartFailure, IssueDispatchMode, IssueRunPlan, PhaseGoalKind,
		RUN_LEASE_IDLE_TIMEOUT, RepoGateFailure, RepoGateFailureKind, ServiceConfig,
		StalledRunNeedsAttention, WorkflowDocument,
		tests::{
			FakeTracker, TEST_SERVICE_ID, {self},
		},
	},
	state::StateStore,
	tracker::{self, records},
	worktree::WorktreeSpec,
};

#[test]
fn retryable_app_server_failures_do_not_write_attention_before_budget_exhaustion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();

	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		1,
		Report::new(AppServerTransportFailure::with_phase(
			String::from("App-server stdout disconnected unexpectedly."),
			"thread/start",
			true,
		)),
		"app_server_transport_disconnected",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		2,
		Report::new(AppServerTurnFailure::new(
			"thread-1",
			Some(String::from("turn-1")),
			"failed",
			"transient model failure",
			None,
		)),
		"retryable_execution_failure",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		3,
		Report::new(AppServerTurnFailure::new(
			"thread-1",
			Some(String::from("turn-1")),
			"failed",
			"You've hit your usage limit.",
			Some(String::from("usageLimitExceeded")),
		)),
		"app_server_usage_limit_exceeded",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		4,
		Report::new(AppServerZeroEvidenceStartFailure::new(
			String::from("PUB-104"),
			String::from("pub-104-attempt-1-123"),
		)),
		"app_server_zero_evidence_start_failed",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		5,
		Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
			"plugin/list",
			String::from("Timed out while waiting for app-server output."),
		)),
		"app_server_plugin_list_timeout",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		6,
		Report::new(AppServerPhaseGoalFailure::missing_terminal_path_for_test(
			PhaseGoalKind::HandoffEvidence,
		)),
		"phase_goal_terminal_path_missing",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		7,
		Report::new(AppServerDynamicToolFailure::protocol_for_test(
			Some(String::from("issue_comment")),
			"dynamic tool declaration was missing input schema",
		)),
		"app_server_dynamic_tool_protocol_failure",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		8,
		Report::new(AppServerDynamicToolFailure::tool_for_test(
			Some(String::from("issue_comment")),
			"tool rejected",
		)),
		"app_server_dynamic_tool_failed",
	);
}

#[test]
fn retryable_orchestrator_failures_do_not_write_attention_before_budget_exhaustion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();

	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		1,
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::GitLockContention,
			String::from("fatal: Unable to create '.git/index.lock': File exists."),
		)),
		"repo_gate_git_lock_contention",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		2,
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			String::from("cargo make check failed."),
		)),
		"repo_gate_verify_failed",
	);
	assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		3,
		Report::new(StalledRunNeedsAttention {
			issue_identifier: String::from("PUB-103"),
			run_id: String::from("pub-103-attempt-1-123"),
			idle_for: RUN_LEASE_IDLE_TIMEOUT + Duration::from_secs(1),
		}),
		"stalled_run_detected",
	);
}

#[test]
fn dirty_retryable_runtime_failures_keep_automatic_recovery_before_budget_exhaustion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();

	assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		1,
		Report::new(AppServerTransportFailure::with_phase(
			String::from("App-server stdout disconnected unexpectedly."),
			"thread/start",
			true,
		)),
		"app_server_transport_disconnected",
	);
	assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		2,
		Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
			"plugin/list",
			String::from("Timed out while waiting for app-server output."),
		)),
		"app_server_plugin_list_timeout",
	);
	assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		3,
		Report::new(AppServerPhaseGoalFailure::missing_terminal_path_for_test(
			PhaseGoalKind::HandoffEvidence,
		)),
		"phase_goal_terminal_path_missing",
	);
	assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		4,
		Report::new(AppServerDynamicToolFailure::protocol_for_test(
			Some(String::from("issue_comment")),
			"dynamic tool declaration was missing input schema",
		)),
		"app_server_dynamic_tool_protocol_failure",
	);
	assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		5,
		Report::new(AppServerDynamicToolFailure::tool_for_test(
			Some(String::from("issue_comment")),
			"tool rejected",
		)),
		"app_server_dynamic_tool_failed",
	);
	assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		6,
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::GitLockContention,
			String::from("fatal: Unable to create '.git/index.lock': File exists."),
		)),
		"repo_gate_git_lock_contention",
	);
	assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		7,
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			String::from("cargo make check failed."),
		)),
		"repo_gate_verify_failed",
	);
}

fn assert_retryable_failure_writeback_does_not_require_attention(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	case_number: usize,
	error: Report,
	expected_error_class: &str,
) {
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_id = format!("issue-{case_number}");
	let issue_identifier = format!("PUB-10{case_number}");
	let issue = tests::sample_issue_with_sort_fields(
		&issue_id,
		&issue_identifier,
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: format!("x/pubfi-{}", issue_identifier.to_lowercase()),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join(&issue.identifier),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: format!("pub-10{case_number}-attempt-1-123"),
		retry_budget_base: 0,
	};

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, config, workflow, &state_store, &issue_run, &error)
		.expect("retryable failure handling should succeed");

	let comments = tracker.comments.borrow();

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(comments.iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains(expected_error_class)
	}));
	assert!(comments.iter().all(|comment| {
		!comment.contains("decodex run failed and needs attention")
			&& !comment.contains("decodex retained partial progress and needs attention")
	}));
	assert!(
		comments
			.iter()
			.all(|comment| { records::parse_linear_execution_event_record(comment).is_none() })
	);
	assert!(
		state_store
			.list_linear_execution_events(config.service_id(), &issue.id)
			.expect("linear execution events should list")
			.is_empty()
	);
}

fn assert_dirty_retryable_failure_writeback_does_not_require_attention(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	case_number: usize,
	error: Report,
	expected_error_class: &str,
) {
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_id = format!("issue-dirty-{case_number}");
	let issue_identifier = format!("PUB-30{case_number}");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue_with_sort_fields(
		&issue_id,
		&issue_identifier,
		"In Progress",
		&[active_label.as_str()],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let branch_name = format!("x/pubfi-{}", issue_identifier.to_lowercase());
	let worktree_rel_path = format!(".worktrees/{issue_identifier}");
	let worktree_path = config.worktree_root().join(&issue_identifier);

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", &branch_name, &worktree_rel_path, "main"],
	);
	fs::write(
		worktree_path.join("README.md"),
		format!("dirty retryable recovery case {case_number}\n"),
	)
	.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name,
			issue_identifier: issue.identifier.clone(),
			path: worktree_path,
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: format!("pub-30{case_number}-attempt-1-123"),
		retry_budget_base: 0,
	};

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, config, workflow, &state_store, &issue_run, &error)
		.expect("dirty retryable failure handling should succeed");

	let comments = tracker.comments.borrow();

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(comments.iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains(expected_error_class)
	}));
	assert!(
		comments.iter().all(|comment| {
			!comment.contains("decodex retained partial progress and needs attention")
				&& !comment.contains("decodex run failed and needs attention")
		}),
		"retained tracked changes must not force manual attention for `{expected_error_class}` while retry budget remains"
	);
	assert!(
		comments
			.iter()
			.all(|comment| { records::parse_linear_execution_event_record(comment).is_none() })
	);
	assert!(
		state_store
			.list_linear_execution_events(config.service_id(), &issue.id)
			.expect("linear execution events should list")
			.is_empty()
	);
}
