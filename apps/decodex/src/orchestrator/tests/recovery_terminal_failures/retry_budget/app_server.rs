use color_eyre::Report;

use crate::{
	agent::{
		AppServerCapabilityPreflightFailure, AppServerDynamicToolFailure,
		AppServerPhaseGoalFailure, AppServerTransportFailure, AppServerTurnFailure,
	},
	orchestrator::{
		AppServerZeroEvidenceStartFailure, PhaseGoalKind, tests,
		tests::recovery_terminal_failures::retry_budget::support,
	},
};

#[test]
fn retryable_app_server_failures_do_not_write_attention_before_budget_exhaustion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();

	support::assert_retryable_failure_writeback_does_not_require_attention(
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
	support::assert_retryable_failure_writeback_does_not_require_attention(
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
		"app_server_turn_failed",
	);
	support::assert_retryable_failure_writeback_does_not_require_attention(
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
	support::assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		4,
		Report::new(AppServerZeroEvidenceStartFailure::new(
			String::from("PUB-104"),
			String::from("pub-104-attempt-1-123"),
		)),
		"app_server_zero_evidence_start_failed",
	);
	support::assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		5,
		Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
			"plugin/list",
			String::from("Timed out while waiting for app-server output."),
		)),
		"app_server_plugin_list_timeout",
	);
	support::assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		6,
		Report::new(AppServerPhaseGoalFailure::missing_terminal_path_for_test(
			PhaseGoalKind::HandoffEvidence,
		)),
		"phase_goal_terminal_path_missing",
	);
	support::assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		7,
		Report::new(AppServerDynamicToolFailure::protocol_for_test(
			Some(String::from("issue_comment")),
			"dynamic tool declaration was missing input schema",
		)),
		"app_server_dynamic_tool_protocol_failure",
	);
	support::assert_retryable_failure_writeback_does_not_require_attention(
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
