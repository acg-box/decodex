use crate::orchestrator::daemon_retry::{
	IssueDispatchMode, PhaseGoalRecoveryContinuation, RetryKind,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetryEntryRetentionDecision {
	Retain,
	Drop,
	Block,
}

pub(crate) enum ChildExitPhaseGoalRecovery {
	None,
	Continuation(PhaseGoalRecoveryContinuation),
	Terminalized,
}

pub(crate) struct ChildExitRetrySchedule<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) continuation_initial_issue_state: Option<String>,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) kind: RetryKind,
	pub(crate) attempt: u32,
}
