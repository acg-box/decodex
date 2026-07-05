use crate::orchestrator::{
	RepoGateFailure,
	tests::runtime_failure::{
		AppServerCapabilityPreflightFailure, AppServerDynamicToolFailure,
		AppServerPhaseGoalFailure, AppServerTransportFailure, AppServerTurnFailure,
		AppServerZeroEvidenceStartFailure, Duration, PhaseGoalKind, RUN_LEASE_IDLE_TIMEOUT,
		RepoGateFailureKind, Report, RunFailureWritebackDisposition, StalledRunNeedsAttention,
		orchestrator,
	},
};

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
