use crate::orchestrator::{
	kernel::command::CommandIntent,
	status::{OperatorQueuedIssueStatus, TrackerIssue},
};

#[derive(Clone, Debug)]
pub(crate) struct QueuedCandidateStatusPlan {
	pub(crate) statuses: Vec<OperatorQueuedIssueStatus>,
	pub(crate) guardrail_commands: Vec<QueuedGuardrailCommand>,
}

#[derive(Clone, Debug)]
pub(crate) struct QueuedGuardrailCommand {
	pub(crate) intent: CommandIntent,
	pub(super) action: QueuedGuardrailCommandAction,
	pub(super) issue: TrackerIssue,
}

pub(super) struct QueuedIssueStatusOutcome {
	pub(super) status: OperatorQueuedIssueStatus,
	pub(super) guardrail_command: Option<QueuedGuardrailCommand>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum QueuedGuardrailCommandAction {
	ObserveDependencyProgramStale,
	ClearDependencyProgramStale,
}
