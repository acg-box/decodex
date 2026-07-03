use std::fs;

use color_eyre::Report;

use crate::{
	agent::{
		AppServerCapabilityPreflightFailure, AppServerHomePreflightFailure,
		AppServerTransportFailure,
	},
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan,
		tests::{
			FakeTracker, recovery_terminal_support, {self},
		},
	},
	state::{self, StateStore},
	worktree::WorktreeSpec,
};

#[test]
fn app_server_failures_skip_retry_and_require_attention() {
	recovery_terminal_support::assert_app_server_failure_requires_attention(
		Report::new(AppServerCapabilityPreflightFailure::blocked_for_test(
			"skills",
			"skills/list returned no enabled skills.",
		)),
		"app_server_runtime_preflight_failed",
		"repair the local Codex runtime configuration",
	);
	recovery_terminal_support::assert_app_server_failure_requires_attention(
		Report::new(AppServerHomePreflightFailure::resolution_failed(String::from(
			"app_server_preflight_failed: HOME is not set, so Decodex cannot resolve the shared Codex home for app-server dispatch.",
		))),
		"app_server_codex_home_preflight_failed",
		"inspect the local Decodex and Codex home sharing",
	);
	recovery_terminal_support::assert_app_server_failure_requires_attention(
		Report::new(AppServerHomePreflightFailure::initialize_mismatch(
			String::from("/tmp/per-account-codex-home"),
			String::from("/Users/test/.codex"),
		)),
		"app_server_codex_home_mismatch",
		"restart `decodex serve`",
	);
	recovery_terminal_support::assert_app_server_failure_requires_attention(
		Report::new(AppServerTransportFailure::new(String::from(
			"App-server stdout disconnected unexpectedly.",
		))),
		"app_server_transport_disconnected",
		"resolve the Codex app-server transport failure manually",
	);
	recovery_terminal_support::assert_app_server_failure_requires_attention(
		Report::new(AppServerTransportFailure::with_phase(
			String::from("App-server stdout disconnected unexpectedly."),
			"turn/start",
			false,
		)),
		"app_server_transport_disconnected",
		"resolve the Codex app-server transport failure during `turn/start` manually",
	);
}

#[test]
fn app_server_preflight_timeouts_retry_before_attention_budget_is_exhausted() {
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
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
		"plugin/list",
		String::from("Timed out while waiting for app-server output."),
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("preflight timeout should remain retryable");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("app_server_plugin_list_timeout")
			&& comment.contains("retry app-server preflight automatically")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention"))
	);

	let marker = state::read_run_activity_marker_snapshot(&issue_run.worktree.path)
		.expect("retry schedule should be readable")
		.expect("retry schedule marker should exist");

	assert_eq!(marker.retry_kind(), Some("failure"));
}

#[test]
fn exhausted_app_server_preflight_timeout_retry_budget_requires_attention_with_timeout_class() {
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
	let error = Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
		"plugin/list",
		String::from("Timed out while waiting for app-server output."),
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("exhausted preflight timeout should require attention");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and needs attention")
			&& comment.contains("app_server_plugin_list_timeout")
			&& comment
				.contains("app_server_preflight_failed evidence for the `plugin/list` timeout")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and will retry"))
	);
}
