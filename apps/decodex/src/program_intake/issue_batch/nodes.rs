use std::collections::{BTreeMap, BTreeSet};

use crate::{
	execution_program::{
		ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionDependencySnapshot,
		ExecutionLinearIssueMapping, ExecutionProgramDependency, ExecutionProgramNode,
		ExecutionProgramNodeStage, ExecutionQueueIntent,
	},
	orchestrator,
	prelude::Result,
	program_intake::issue_batch::identity,
	program_intake::model::IssueFacts,
	tracker::{self, IssueTracker, TrackerIssue},
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

pub(in crate::program_intake) fn issue_facts<T>(
	tracker: &T,
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
	active_label: &str,
) -> Result<IssueFacts>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();
	let has_active_label =
		tracker::issue_has_label_with_server_confirmation(tracker, issue, active_label)?;
	let has_opt_out_label = tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		tracker_policy.opt_out_label(),
	)?;
	let has_needs_attention_label = tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		tracker_policy.needs_attention_label(),
	)?;
	let has_open_blockers =
		issue.blockers.iter().any(|blocker| !state_name_is_terminal(&blocker.state.name, workflow));

	Ok(IssueFacts {
		has_active_label,
		has_opt_out_label,
		has_needs_attention_label,
		has_generic_dispatch_briefing: issue_has_generic_dispatch_briefing(issue),
		has_open_blockers,
	})
}

pub(in crate::program_intake) fn issue_node(
	issue: &TrackerIssue,
	facts: &IssueFacts,
	workflow: &WorkflowDocument,
	supplied_node_ids: &BTreeMap<String, String>,
) -> Result<ExecutionProgramNode> {
	let queue_intent = issue_queue_intent(issue, facts, workflow);
	let mut mapping =
		ExecutionLinearIssueMapping::new(&issue.id, &issue.identifier, &issue.state.name)?;

	mapping = mapping
		.with_active_label(facts.has_active_label)
		.with_opt_out_label(facts.has_opt_out_label)
		.with_needs_attention_label(facts.has_needs_attention_label)
		.with_open_tracker_blockers(facts.has_open_blockers)
		.with_generic_dispatch_briefing(facts.has_generic_dispatch_briefing);

	ExecutionProgramNode::new(
		identity::node_id_for_issue(&issue.identifier),
		ExecutionProgramNodeStage::Runtime,
		issue.title.clone(),
		queue_intent,
	)?
	.with_objective_lineage([format!("Issue-batch intake supplied `{}`.", issue.identifier)])?
	.with_dependencies(issue_dependencies(issue, supplied_node_ids)?)?
	.with_conflict_domains(issue_conflict_domains(issue)?)?
	.with_acceptance_expectations([format!(
		"`{}` remains a normal Linear issue with an executable brief.",
		issue.identifier
	)])?
	.with_validation_expectations([String::from(
		"Run the issue-specific repository validation before review handoff.",
	)])?
	.with_linear_issue(mapping)
}

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

pub(in crate::program_intake) fn issue_dependencies(
	issue: &TrackerIssue,
	supplied_node_ids: &BTreeMap<String, String>,
) -> Result<Vec<ExecutionProgramDependency>> {
	let mut dependencies = BTreeMap::new();

	for blocker in &issue.blockers {
		let dependency_id = supplied_node_ids
			.get(&blocker.identifier)
			.cloned()
			.unwrap_or_else(|| blocker.identifier.clone());

		dependencies
			.entry(dependency_id.clone())
			.or_insert(ExecutionProgramDependency::new(dependency_id)?);
	}

	Ok(dependencies.into_values().collect())
}

pub(in crate::program_intake) fn dependency_snapshots_for(
	issue: &TrackerIssue,
	supplied_node_ids: &BTreeMap<String, String>,
) -> Result<Vec<ExecutionDependencySnapshot>> {
	let mut snapshots = BTreeMap::new();

	for blocker in &issue.blockers {
		let dependency_id = supplied_node_ids
			.get(&blocker.identifier)
			.cloned()
			.unwrap_or_else(|| blocker.identifier.clone());
		let snapshot = ExecutionDependencySnapshot::tracker_state(
			dependency_id.clone(),
			blocker.state.name.clone(),
		)?;

		snapshots.entry(dependency_id).or_insert(snapshot);
	}

	Ok(snapshots.into_values().collect())
}

pub(in crate::program_intake) fn issue_conflict_domains(
	issue: &TrackerIssue,
) -> Result<Vec<ExecutionConflictDomain>> {
	let mut domains = vec![ExecutionConflictDomain::new(
		ExecutionConflictDomainKind::TrackerOwnership,
		issue.identifier.clone(),
	)?];
	let mut seen = BTreeSet::from([format!(
		"{}:{}",
		ExecutionConflictDomainKind::TrackerOwnership.as_str(),
		issue.identifier
	)]);

	for label in &issue.labels {
		if let Some(module) = label.name.strip_prefix("repo:")
			&& !module.trim().is_empty()
		{
			let key = module.trim().to_owned();
			let seen_key = format!("{}:{key}", ExecutionConflictDomainKind::Module.as_str());

			if seen.insert(seen_key) {
				domains
					.push(ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, key)?);
			}
		}
	}

	domains.sort_by(|left, right| {
		left.kind().as_str().cmp(right.kind().as_str()).then_with(|| left.key().cmp(right.key()))
	});

	Ok(domains)
}

pub(in crate::program_intake) fn state_name_is_terminal(
	state_name: &str,
	workflow: &WorkflowDocument,
) -> bool {
	workflow.frontmatter().tracker().terminal_states().iter().any(|state| state == state_name)
}

pub(in crate::program_intake) fn issue_has_generic_dispatch_briefing(issue: &TrackerIssue) -> bool {
	orchestrator::issue_has_generic_dispatch_briefing(issue)
}
