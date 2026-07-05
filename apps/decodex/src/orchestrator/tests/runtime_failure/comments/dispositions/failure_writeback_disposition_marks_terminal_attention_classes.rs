use crate::orchestrator::{
	RepoGateFailure,
	tests::runtime_failure::{
		AppServerCapabilityPreflightFailure, AppServerPhaseGoalFailure, AppServerTransportFailure,
		AppServerTurnFailure, CodexAccountAuthFailure, RepoGateFailureKind, Report,
		RunFailureWritebackDisposition, orchestrator,
	},
};

#[test]
fn failure_writeback_disposition_marks_terminal_attention_classes() {
	for (case_name, error, expected_disposition) in [
		(
			"turn transport",
			Report::new(AppServerTransportFailure::with_phase(
				String::from("App-server stdout disconnected during turn start."),
				"turn/start",
				false,
			)),
			RunFailureWritebackDisposition::TerminalAttention,
		),
		(
			"operator attention turn failure",
			Report::new(AppServerTurnFailure::new(
				"thread-1",
				Some(String::from("turn-1")),
				"failed",
				"operator attention required",
				Some(String::from("operatorAttentionRequired")),
			)),
			RunFailureWritebackDisposition::TerminalAttention,
		),
		(
			"app-server capability preflight blocker",
			Report::new(AppServerCapabilityPreflightFailure::blocked_for_test(
				"model",
				"configured model was not present in model/list.",
			)),
			RunFailureWritebackDisposition::TerminalAttention,
		),
		(
			"unsupported phase goal API",
			Report::new(AppServerPhaseGoalFailure::unsupported_for_test("thread/goal/set")),
			RunFailureWritebackDisposition::TerminalAttention,
		),
		(
			"repo gate spawn failure",
			Report::new(RepoGateFailure::new(
				RepoGateFailureKind::CommandSpawnFailed,
				String::from("Failed to spawn repo gate command `cargo make test`: missing tool"),
			)),
			RunFailureWritebackDisposition::TerminalAttention,
		),
		(
			"codex account auth failure",
			Report::new(CodexAccountAuthFailure::new(
				Some(String::from("...123456")),
				Some(String::from("bad@example.com")),
				"Codex account `bad@example.com` token refresh failed with HTTP 401 Unauthorized.",
			)),
			RunFailureWritebackDisposition::TerminalAttention,
		),
	] {
		assert_eq!(
			orchestrator::run_failure_writeback_disposition(&error),
			expected_disposition,
			"{case_name}"
		);
	}
}
