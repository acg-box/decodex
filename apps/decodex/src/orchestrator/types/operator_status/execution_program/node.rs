use serde::{Deserialize, Serialize};

use crate::{
	execution_program::{ExecutionNodeEvaluation, ExecutionProgramNodeLifecycleState},
	orchestrator::types::operator_status::execution_program::{reason, status},
	state::ExecutionProgramRecord,
};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorExecutionProgramNodeStatus {
	#[serde(default = "status::operator_execution_program_unknown_status")]
	pub(crate) program_stage: String,
	pub(crate) lifecycle_state: String,
	pub(crate) readiness_state: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) issue_state: Option<String>,
	pub(crate) dispatch_action: Option<String>,
	pub(crate) reason_codes: Vec<String>,
	pub(crate) reasons: Vec<String>,
	pub(crate) next_action: String,
}

pub(crate) fn operator_execution_program_node_should_render(
	node: &ExecutionNodeEvaluation,
) -> bool {
	node.dispatch_action().is_some()
		|| matches!(
			node.lifecycle_state(),
			ExecutionProgramNodeLifecycleState::Active
				| ExecutionProgramNodeLifecycleState::Blocked
				| ExecutionProgramNodeLifecycleState::Mapped
				| ExecutionProgramNodeLifecycleState::NeedsAttention
				| ExecutionProgramNodeLifecycleState::PostReview
				| ExecutionProgramNodeLifecycleState::Planned
				| ExecutionProgramNodeLifecycleState::Stale
				| ExecutionProgramNodeLifecycleState::Superseded
		)
}

pub(crate) fn operator_execution_program_node_readback(
	node: &ExecutionNodeEvaluation,
) -> OperatorExecutionProgramNodeStatus {
	let reason_codes = reason::operator_execution_program_reason_codes(node.reasons());
	let reasons = node
		.reasons()
		.iter()
		.map(|program_reason| reason::operator_execution_program_public_reason(program_reason))
		.collect::<Vec<_>>();
	let issue = node.linear_issue();

	OperatorExecutionProgramNodeStatus {
		program_stage: node.stage().as_str().to_owned(),
		lifecycle_state: node.lifecycle_state().as_str().to_owned(),
		readiness_state: node.state().as_str().to_owned(),
		issue_identifier: issue.map(|issue| issue.issue_identifier().to_owned()),
		issue_state: issue.map(|issue| issue.issue_state().to_owned()),
		dispatch_action: node.dispatch_action().map(|action| action.as_str().to_owned()),
		next_action: operator_execution_program_node_next_action(node, &reason_codes),
		reason_codes,
		reasons,
	}
}

pub(crate) fn operator_execution_program_missing_contract_nodes(
	record: &ExecutionProgramRecord,
) -> Vec<OperatorExecutionProgramNodeStatus> {
	record
		.program()
		.nodes()
		.iter()
		.map(|node| {
			let issue = node.linear_issue();

			OperatorExecutionProgramNodeStatus {
				program_stage: node.stage().as_str().to_owned(),
				lifecycle_state: String::from("stale"),
				readiness_state: String::from("stale"),
				issue_identifier: issue.map(|issue| issue.issue_identifier().to_owned()),
				issue_state: issue.map(|issue| issue.issue_state().to_owned()),
				dispatch_action: None,
				reason_codes: vec![String::from("source_decision_contract_missing")],
				reasons: vec![String::from("source Decision Contract is missing")],
				next_action: String::from(
					"Restore or supersede the source Decision Contract before dispatching this program.",
				),
			}
		})
		.collect()
}

pub(crate) fn operator_execution_program_node_next_action(
	node: &ExecutionNodeEvaluation,
	reason_codes: &[String],
) -> String {
	if matches!(
		node.lifecycle_state(),
		ExecutionProgramNodeLifecycleState::Stale | ExecutionProgramNodeLifecycleState::Superseded
	) {
		return String::from(
			"Refresh or supersede the accepted Decision Contract before dispatching this program.",
		);
	}
	if reason_codes.iter().any(|code| code == "dependency_not_terminal") {
		return String::from(
			"Complete the dependency issue or refresh the Execution Program dependency plan if this remains stale.",
		);
	}
	if matches!(node.lifecycle_state(), ExecutionProgramNodeLifecycleState::NeedsAttention)
		|| reason_codes.iter().any(|code| code == "mapped_issue_needs_attention")
	{
		return String::from(
			"Resolve the mapped issue's needs-attention stop before dispatching this node.",
		);
	}
	if matches!(node.lifecycle_state(), ExecutionProgramNodeLifecycleState::Active) {
		return String::from(
			"Wait for the current lane or recover its retained state before dispatching this node.",
		);
	}
	if matches!(node.lifecycle_state(), ExecutionProgramNodeLifecycleState::PostReview)
		|| reason_codes.iter().any(|code| code == "mapped_issue_post_review_owner")
	{
		return String::from(
			"Continue the retained post-review lifecycle before dispatching this program node.",
		);
	}
	if node.dispatch_action().is_some() {
		return String::from("The program scheduler can dispatch this node directly.");
	}
	if matches!(
		node.lifecycle_state(),
		ExecutionProgramNodeLifecycleState::Planned | ExecutionProgramNodeLifecycleState::Mapped
	) {
		return String::from("Map, promote, or unpause this intake node before dispatching it.");
	}
	if matches!(node.lifecycle_state(), ExecutionProgramNodeLifecycleState::Blocked) {
		return String::from(
			"Repair mapped issue blockers, briefing, or program readiness before retrying.",
		);
	}

	String::from("No operator action required.")
}
