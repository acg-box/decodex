use color_eyre::Report;

use crate::{
	agent::{
		AppServerCapabilityPreflightFailure, AppServerDynamicToolFailure,
		AppServerPhaseGoalFailure, AppServerTransportFailure,
	},
	orchestrator::{
		PhaseGoalKind, RepoGateFailure, RepoGateFailureKind, tests,
		tests::recovery_terminal_failures::retry_budget::support,
	},
};

#[test]
fn keeps_automatic_recovery_before_budget_exhaustion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();

	support::assert_dirty_retryable_failure_writeback_does_not_require_attention(
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
	support::assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		2,
		Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
			"plugin/list",
			String::from("Timed out while waiting for app-server output."),
		)),
		"app_server_plugin_list_timeout",
	);
	support::assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		3,
		Report::new(AppServerPhaseGoalFailure::missing_terminal_path_for_test(
			PhaseGoalKind::HandoffEvidence,
		)),
		"phase_goal_terminal_path_missing",
	);
	support::assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		4,
		Report::new(AppServerDynamicToolFailure::protocol_for_test(
			Some(String::from("issue_comment")),
			"dynamic tool declaration was missing input schema",
		)),
		"app_server_dynamic_tool_protocol_failure",
	);
	support::assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		5,
		Report::new(AppServerDynamicToolFailure::tool_for_test(
			Some(String::from("issue_comment")),
			"tool rejected",
		)),
		"app_server_dynamic_tool_failed",
	);
	support::assert_dirty_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		6,
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::GitLockContention,
			String::from("fatal: Unable to create '.git/index.lock': File exists."),
		)),
		"repo_gate_git_lock_contention",
	);
	support::assert_dirty_retryable_failure_writeback_does_not_require_attention(
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
