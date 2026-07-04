use crate::execution_program::{
	model::{
		ExecutionDispatchAction, ExecutionProgramNode, ExecutionProgramNodeLifecycleState,
		ExecutionQueueIntent, ExecutionReadinessState,
	},
	policy::ExecutionWorkflowPolicy,
};

pub(super) fn dispatch_action_for(
	node: &ExecutionProgramNode,
	state: ExecutionReadinessState,
	policy: &ExecutionWorkflowPolicy,
) -> Option<ExecutionDispatchAction> {
	let issue = node.linear_issue()?;

	if state != ExecutionReadinessState::Ready {
		return None;
	}
	if !matches!(
		node.queue_intent(),
		ExecutionQueueIntent::ReadyToQueue | ExecutionQueueIntent::Queued
	) || !policy.issue_is_startable(issue)
	{
		return None;
	}

	Some(ExecutionDispatchAction::Dispatch)
}

pub(super) fn lifecycle_state_for(
	node: &ExecutionProgramNode,
	state: ExecutionReadinessState,
) -> ExecutionProgramNodeLifecycleState {
	if let Some(issue) = node.linear_issue()
		&& issue.has_needs_attention_label
	{
		return ExecutionProgramNodeLifecycleState::NeedsAttention;
	}
	if let Some(issue) = node.linear_issue()
		&& issue.has_post_review_lifecycle
	{
		return ExecutionProgramNodeLifecycleState::PostReview;
	}
	if let Some(issue) = node.linear_issue()
		&& issue.has_active_label
	{
		return ExecutionProgramNodeLifecycleState::Active;
	}

	match state {
		ExecutionReadinessState::NotReady | ExecutionReadinessState::Paused => {
			if node.linear_issue().is_some() {
				ExecutionProgramNodeLifecycleState::Mapped
			} else {
				ExecutionProgramNodeLifecycleState::Planned
			}
		},
		ExecutionReadinessState::Ready => ExecutionProgramNodeLifecycleState::Ready,
		ExecutionReadinessState::Blocked => ExecutionProgramNodeLifecycleState::Blocked,
		ExecutionReadinessState::Active => ExecutionProgramNodeLifecycleState::Active,
		ExecutionReadinessState::Completed => ExecutionProgramNodeLifecycleState::Completed,
		ExecutionReadinessState::Stale => ExecutionProgramNodeLifecycleState::Stale,
	}
}
