//! Per-node readiness evaluation.

mod blocking;
mod dispatch;
mod model;

pub(crate) use self::model::{EvaluateNodeInput, ExecutionNodeEvaluation};

use crate::{
	execution_program::model::{
		ExecutionProgramNodeLifecycleState, ExecutionQueueIntent, ExecutionReadinessState,
	},
	loop_contract::DecisionContractStatus,
	prelude::Result,
};

pub(crate) fn evaluate_node(input: EvaluateNodeInput<'_>) -> Result<ExecutionNodeEvaluation> {
	let EvaluateNodeInput {
		program,
		node,
		current_contract,
		current_fingerprint,
		policy,
		node_lookup,
		dependency_lookup,
		occupied_conflicts,
		active_issue_ids,
	} = input;
	let authority_matches =
		current_contract.map_or(program.source_contract_id.is_none(), |contract| {
			contract.status() == DecisionContractStatus::AcceptedPromoted
				&& Some(contract.contract_id()) == program.source_contract_id.as_deref()
		});
	let mut reasons = Vec::new();
	let mut state = ExecutionReadinessState::Ready;
	let mut lifecycle_state = None;

	if let Some(issue) = node.linear_issue()
		&& policy.issue_is_terminal(issue)
	{
		state = ExecutionReadinessState::Completed;
		lifecycle_state = Some(ExecutionProgramNodeLifecycleState::Completed);

		reasons.push(format!(
			"mapped issue `{}` is already terminal in `{}`",
			issue.issue_identifier(),
			issue.issue_state()
		));
	} else if !authority_matches
		|| current_fingerprint != program.accepted_contract_fingerprint
		|| current_fingerprint != node.contract_fingerprint
	{
		state = ExecutionReadinessState::Stale;
		lifecycle_state = Some(
			if current_contract.is_some_and(|contract| {
				contract.status() == DecisionContractStatus::RejectedSuperseded
			}) {
				ExecutionProgramNodeLifecycleState::Superseded
			} else {
				ExecutionProgramNodeLifecycleState::Stale
			},
		);

		reasons.push(String::from("node no longer matches the accepted Decision Contract"));
	} else if let Some(issue) = node.linear_issue()
		&& active_issue_ids.contains(issue.issue_id())
		&& !issue.has_opt_out_label()
		&& !issue.has_needs_attention_label()
		&& !issue.has_post_review_lifecycle()
	{
		state = ExecutionReadinessState::Active;
		lifecycle_state = Some(ExecutionProgramNodeLifecycleState::Active);

		reasons.push(String::from("node already has a current lane"));
	} else {
		match node.queue_intent {
			ExecutionQueueIntent::NotReady => {
				state = ExecutionReadinessState::NotReady;

				reasons.push(String::from("node dispatch intent is not-ready"));
			},
			ExecutionQueueIntent::Paused => {
				state = ExecutionReadinessState::Paused;

				reasons.push(String::from("node dispatch intent is paused"));
			},
			ExecutionQueueIntent::Active => {
				state = ExecutionReadinessState::Active;

				reasons.push(String::from("node already has a current lane"));
			},
			ExecutionQueueIntent::Done | ExecutionQueueIntent::Canceled => {
				state = ExecutionReadinessState::Completed;

				reasons.push(String::from("node dispatch intent is terminal"));
			},
			ExecutionQueueIntent::ReadyToQueue | ExecutionQueueIntent::Queued => {
				blocking::collect_blocking_readiness_reasons(
					node,
					policy,
					node_lookup,
					dependency_lookup,
					occupied_conflicts,
					&mut reasons,
				);

				if !reasons.is_empty() {
					state = ExecutionReadinessState::Blocked;
				}
			},
		}
	}

	if state == ExecutionReadinessState::Ready {
		reasons.push(String::from("node is ready for normal Linear issue execution"));
	}

	let dispatch_action = dispatch::dispatch_action_for(node, state, policy);
	let lifecycle_state =
		lifecycle_state.unwrap_or_else(|| dispatch::lifecycle_state_for(node, state));

	Ok(ExecutionNodeEvaluation {
		node_id: node.node_id.clone(),
		stage: node.stage,
		state,
		lifecycle_state,
		reasons,
		dispatch_action,
		linear_issue: node.linear_issue.clone(),
	})
}
