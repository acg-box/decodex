use crate::orchestrator::{RunAttempt, daemon_retry::RetryKind};

pub(crate) fn continuation_child_exit_retry_plan(
	run_attempt: &RunAttempt,
	initial_issue_state: &str,
) -> (RetryKind, u32, Option<String>) {
	(
		RetryKind::Continuation,
		u32::try_from(run_attempt.attempt_number()).unwrap_or(u32::MAX).max(1),
		Some(initial_issue_state.to_owned()),
	)
}
