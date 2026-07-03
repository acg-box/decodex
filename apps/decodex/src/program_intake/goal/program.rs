use crate::{
	execution_program::{
		ExecutionLinearIssueMapping, ExecutionProgram, ExecutionProgramDependency,
		ExecutionProgramNode,
	},
	loop_contract::DecisionContract,
	prelude::Result,
	program_intake::{goal, issue_batch::nodes, model::GoalIssuePlan},
	tracker::{self, TrackerIssue},
	workflow::WorkflowDocument,
};

pub(in crate::program_intake) fn goal_execution_program(
	service_id: &str,
	program_id: &str,
	contract: &DecisionContract,
	plans: &[GoalIssuePlan],
	issues: &[TrackerIssue],
	workflow: &WorkflowDocument,
) -> Result<ExecutionProgram> {
	let nodes = plans
		.iter()
		.zip(issues)
		.map(|(plan, issue)| goal_program_node(service_id, contract, plan, issue, workflow))
		.collect::<Result<Vec<_>>>()?;

	ExecutionProgram::from_accepted_contract(program_id, service_id, contract, nodes)
}

pub(in crate::program_intake) fn goal_program_node(
	service_id: &str,
	contract: &DecisionContract,
	plan: &GoalIssuePlan,
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> Result<ExecutionProgramNode> {
	let dependencies = plan
		.dependency_node_ids
		.iter()
		.map(ExecutionProgramDependency::new)
		.collect::<Result<Vec<_>>>()?;
	let mapping = goal_issue_mapping(service_id, issue, workflow)?;

	ExecutionProgramNode::new(
		plan.node_id.clone(),
		plan.stage,
		plan.objective.clone(),
		plan.queue_intent,
	)?
	.with_objective_lineage(goal::goal_objective_lineage(contract))?
	.with_dependencies(dependencies)?
	.with_conflict_domains(plan.conflict_domains.clone())?
	.with_acceptance_expectations(plan.acceptance.clone())?
	.with_validation_expectations(plan.validation.clone())?
	.with_linear_issue(mapping)
}

pub(in crate::program_intake) fn goal_issue_mapping(
	service_id: &str,
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> Result<ExecutionLinearIssueMapping> {
	let active_label = tracker::automation_active_label(service_id);
	let tracker_policy = workflow.frontmatter().tracker();

	Ok(ExecutionLinearIssueMapping::new(&issue.id, &issue.identifier, &issue.state.name)?
		.with_active_label(issue.has_label(&active_label))
		.with_opt_out_label(issue.has_label(tracker_policy.opt_out_label()))
		.with_needs_attention_label(issue.has_label(tracker_policy.needs_attention_label()))
		.with_generic_dispatch_briefing(nodes::issue_has_generic_dispatch_briefing(issue)))
}
