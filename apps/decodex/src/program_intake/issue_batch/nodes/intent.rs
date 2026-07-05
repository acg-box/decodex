use crate::{
	execution_program::ExecutionQueueIntent, program_intake::model::IssueFacts,
	tracker::TrackerIssue, workflow::WorkflowDocument,
};

pub(in crate::program_intake) fn issue_queue_intent(
	issue: &TrackerIssue,
	facts: &IssueFacts,
	workflow: &WorkflowDocument,
) -> ExecutionQueueIntent {
	if state_name_is_terminal(&issue.state.name, workflow) {
		return ExecutionQueueIntent::Done;
	}
	if facts.has_active_label {
		return ExecutionQueueIntent::Active;
	}
	if facts.has_opt_out_label {
		return ExecutionQueueIntent::NotReady;
	}
	if !workflow
		.frontmatter()
		.tracker()
		.startable_states()
		.iter()
		.any(|state| state == &issue.state.name)
	{
		return ExecutionQueueIntent::NotReady;
	}

	ExecutionQueueIntent::ReadyToQueue
}

pub(in crate::program_intake) fn state_name_is_terminal(
	state_name: &str,
	workflow: &WorkflowDocument,
) -> bool {
	workflow.frontmatter().tracker().terminal_states().iter().any(|state| state == state_name)
}
