use crate::orchestrator::tests::runtime_failure::{
	AppServerCapabilityPreflightFailure, AppServerHomePreflightFailure, AppServerPhaseGoalFailure,
	AppServerTransportFailure, AppServerTurnFailure, BTreeMap, CodexAccountAuthFailure,
	PhaseGoalKind, Report,
	orchestrator::{self, AppServerZeroEvidenceStartFailure},
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
