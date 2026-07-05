use std::time::Duration;

use color_eyre::Report;

use crate::orchestrator::{
	RUN_LEASE_IDLE_TIMEOUT, RepoGateFailure, RepoGateFailureKind, StalledRunNeedsAttention, tests,
	tests::recovery_terminal_failures::retry_budget::support,
};

#[test]
fn retryable_orchestrator_failures_do_not_write_attention_before_budget_exhaustion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();

	support::assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		1,
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::GitLockContention,
			String::from("fatal: Unable to create '.git/index.lock': File exists."),
		)),
		"repo_gate_git_lock_contention",
	);
	support::assert_retryable_failure_writeback_does_not_require_attention(
		&config,
		&workflow,
		2,
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			String::from("cargo make check failed."),
		)),
		"repo_gate_verify_failed",
	);
	support::assert_retryable_failure_writeback_does_not_require_attention(
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
