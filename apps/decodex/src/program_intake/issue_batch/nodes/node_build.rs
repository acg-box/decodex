use std::collections::BTreeMap;

use crate::{
	execution_program::{
		ExecutionLinearIssueMapping, ExecutionProgramNode, ExecutionProgramNodeStage,
		ExecutionQueueIntent,
	},
	prelude::Result,
	program_intake::{
		issue_batch::{identity, nodes},
		model::IssueFacts,
	},
	tracker::TrackerIssue,
	workflow::WorkflowDocument,
};

pub(in crate::program_intake) fn unmapped_node(identifier: &str) -> Result<ExecutionProgramNode> {
	ExecutionProgramNode::new(
		format!("unmapped:{identifier}"),
		ExecutionProgramNodeStage::Runtime,
		format!("Resolve supplied Linear issue identifier `{identifier}` before dispatch."),
		ExecutionQueueIntent::NotReady,
	)?
	.with_acceptance_expectations([format!(
		"`{identifier}` maps to a normal Linear issue before execution."
	)])?
	.with_validation_expectations([String::from("Tracker lookup succeeds before queue intent.")])
}

pub(in crate::program_intake) fn issue_node(
	issue: &TrackerIssue,
	facts: &IssueFacts,
	workflow: &WorkflowDocument,
	supplied_node_ids: &BTreeMap<String, String>,
) -> Result<ExecutionProgramNode> {
	let queue_intent = nodes::issue_queue_intent(issue, facts, workflow);
	let mapping = issue_linear_mapping(issue, facts)?;

	ExecutionProgramNode::new(
		identity::node_id_for_issue(&issue.identifier),
		ExecutionProgramNodeStage::Runtime,
		issue.title.clone(),
		queue_intent,
	)?
	.with_objective_lineage([format!("Issue-batch intake supplied `{}`.", issue.identifier)])?
	.with_dependencies(nodes::issue_dependencies(issue, supplied_node_ids)?)?
	.with_conflict_domains(nodes::issue_conflict_domains(issue)?)?
	.with_acceptance_expectations([format!(
		"`{}` remains a normal Linear issue with an executable brief.",
		issue.identifier
	)])?
	.with_validation_expectations([String::from(
		"Run the issue-specific repository validation before review handoff.",
	)])?
	.with_linear_issue(mapping)
}

fn issue_linear_mapping(
	issue: &TrackerIssue,
	facts: &IssueFacts,
) -> Result<ExecutionLinearIssueMapping> {
	Ok(ExecutionLinearIssueMapping::new(&issue.id, &issue.identifier, &issue.state.name)?
		.with_active_label(facts.has_active_label)
		.with_opt_out_label(facts.has_opt_out_label)
		.with_needs_attention_label(facts.has_needs_attention_label)
		.with_open_tracker_blockers(facts.has_open_blockers)
		.with_generic_dispatch_briefing(facts.has_generic_dispatch_briefing))
}
