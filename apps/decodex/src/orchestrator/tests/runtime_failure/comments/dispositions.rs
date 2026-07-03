use crate::orchestrator::{
	RepoGateFailure,
	tests::runtime_failure::{
		AppServerCapabilityPreflightFailure, AppServerDynamicToolFailure,
		AppServerPhaseGoalFailure, AppServerTransportFailure, AppServerTurnFailure,
		AppServerZeroEvidenceStartFailure, CodexAccountAuthFailure, Duration, PhaseGoalKind,
		RUN_LEASE_IDLE_TIMEOUT, RepoGateFailureKind, Report, RunFailureWritebackDisposition,
		StalledRunNeedsAttention, orchestrator,
	},
};

#[test]
fn terminal_failure_comments_surface_actionable_error_classes() {
	for (error_class, next_action, expected_snippets) in [
		(
			"human_attention_required",
			"inspect the issue comment and worktree, resolve the blocker manually, clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
			&["inspect the issue comment and worktree", "resolve the blocker manually"][..],
		),
		(
			"review_handoff_writeback_failed",
			"inspect the tracker state, PR, and worktree, repair the incomplete review handoff manually, clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
			&["repair the incomplete review handoff manually"][..],
		),
		(
			"stalled_run_detected",
			"inspect the worktree and app-server activity for the stalled lane, resolve the blocker manually, `decodex:needs-attention` could not be applied because it does not exist on the team; the issue remains in `In Progress` to block automatic retries, so move it back to a startable state manually if another automated run is desired",
			&["does not exist on the team", "remains in `In Progress`"][..],
		),
	] {
		let comment = orchestrator::format_terminal_failure_comment(
			"pub-101-attempt-1-123",
			1,
			String::from(".worktrees/PUB-101"),
			"x/pubfi-pub-101",
			None,
			error_class,
			next_action,
		);

		assert!(comment.contains(&format!("- error_class: `{error_class}`")));
		assert!(comment.contains("Sensitive runtime details were withheld"));

		for expected_snippet in expected_snippets {
			assert!(comment.contains(expected_snippet), "{error_class} missing {expected_snippet}");
		}
	}
}

#[test]
fn failure_writeback_disposition_marks_retryable_recovery_classes() {
	for (case_name, error, expected_disposition) in [
		(
			"startup transport",
			Report::new(AppServerTransportFailure::with_phase(
				String::from("App-server stdout disconnected before thread start."),
				"thread/start",
				true,
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"generic turn failure",
			Report::new(AppServerTurnFailure::new(
				"thread-1",
				Some(String::from("turn-1")),
				"failed",
				"transient model failure",
				None,
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"usage limit turn failure",
			Report::new(AppServerTurnFailure::new(
				"thread-1",
				Some(String::from("turn-1")),
				"failed",
				"You've hit your usage limit.",
				Some(String::from("usageLimitExceeded")),
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"repo gate lock contention",
			Report::new(RepoGateFailure::new(
				RepoGateFailureKind::GitLockContention,
				String::from("fatal: Unable to create '.git/index.lock': File exists."),
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"zero-evidence app-server start failure",
			Report::new(AppServerZeroEvidenceStartFailure::new(
				String::from("PUB-101"),
				String::from("pub-101-attempt-1-123"),
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"app-server capability preflight timeout",
			Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
				"plugin/list",
				String::from("Timed out while waiting for app-server output."),
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"phase goal terminal path missing",
			Report::new(AppServerPhaseGoalFailure::missing_terminal_path_for_test(
				PhaseGoalKind::HandoffEvidence,
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"dynamic tool protocol failure",
			Report::new(AppServerDynamicToolFailure::protocol_for_test(
				Some(String::from("issue_comment")),
				"dynamic tool declaration was missing input schema",
			)),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
		(
			"stalled current lane",
			Report::new(StalledRunNeedsAttention {
				issue_identifier: String::from("PUB-101"),
				run_id: String::from("pub-101-attempt-1-123"),
				idle_for: RUN_LEASE_IDLE_TIMEOUT + Duration::from_secs(1),
			}),
			RunFailureWritebackDisposition::RetryableStructuredRecovery,
		),
	] {
		assert_eq!(
			orchestrator::run_failure_writeback_disposition(&error),
			expected_disposition,
			"{case_name}"
		);
	}
}

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
