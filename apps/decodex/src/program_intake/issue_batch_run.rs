use std::collections::BTreeMap;

use crate::{
	config::ServiceConfig,
	execution_program::{
		ExecutionProgram, ExecutionProgramReadinessContext, ExecutionWorkflowPolicy,
	},
	prelude::{Result, eyre},
	program_intake::{IssueBatchIntakeReport, issue_batch},
	state::StateStore,
	tracker::{self, IssueTracker},
	workflow::WorkflowDocument,
};

/// Build and optionally persist a non-mutating issue-batch intake report.
pub(crate) fn run_issue_batch_intake<T>(
	state_store: &StateStore,
	tracker: &T,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue_identifiers: Vec<String>,
	dry_run: bool,
	persist: bool,
) -> Result<IssueBatchIntakeReport>
where
	T: IssueTracker + ?Sized,
{
	if dry_run == persist {
		eyre::bail!("Issue-batch intake requires exactly one of dry_run or persist.");
	}

	let issue_identifiers = issue_batch::normalize_issue_identifiers(issue_identifiers)?;
	let active_label = tracker::automation_active_label(config.service_id());
	let policy = ExecutionWorkflowPolicy::from_workflow(config.service_id(), workflow)?;
	let mut resolved = BTreeMap::new();
	let mut missing = Vec::new();

	for identifier in &issue_identifiers {
		match tracker.get_issue_by_identifier(identifier)? {
			Some(issue) => {
				resolved.insert(identifier.clone(), issue);
			},
			None => missing.push(identifier.clone()),
		}
	}

	let batch_fingerprint =
		issue_batch::issue_batch_fingerprint(config.service_id(), &issue_identifiers, &resolved);
	let program_id = issue_batch::issue_batch_program_id(config.service_id(), &batch_fingerprint);
	let supplied_node_ids = issue_identifiers
		.iter()
		.map(|identifier| (identifier.clone(), issue_batch::node_id_for_issue(identifier)))
		.collect::<BTreeMap<_, _>>();
	let mut nodes = Vec::new();
	let mut dependency_snapshots = Vec::new();
	let mut facts_by_identifier = BTreeMap::new();

	for identifier in &issue_identifiers {
		if let Some(issue) = resolved.get(identifier) {
			let facts = issue_batch::issue_facts(tracker, workflow, issue, &active_label)?;

			dependency_snapshots
				.extend(issue_batch::dependency_snapshots_for(issue, &supplied_node_ids)?);
			nodes.push(issue_batch::issue_node(issue, &facts, workflow, &supplied_node_ids)?);
			facts_by_identifier.insert(identifier.clone(), facts);
		} else {
			nodes.push(issue_batch::unmapped_node(identifier)?);
		}
	}

	let program = ExecutionProgram::from_issue_batch_intake(
		&program_id,
		config.service_id(),
		&batch_fingerprint,
		format!("Issue-batch intake for {} issue(s).", issue_identifiers.len()),
		nodes,
	)?;
	let context =
		ExecutionProgramReadinessContext::new().with_dependency_snapshots(dependency_snapshots);
	let evaluation = program.evaluate_issue_batch(&policy, &context)?;
	let evaluation_by_issue = evaluation
		.nodes()
		.iter()
		.filter_map(|node| {
			let issue = node.linear_issue()?;

			Some((issue.issue_identifier().to_owned(), node))
		})
		.collect::<BTreeMap<_, _>>();
	let mut rows = Vec::new();

	for identifier in &issue_identifiers {
		if missing.iter().any(|missing| missing == identifier) {
			rows.push(issue_batch::unmapped_report_row(identifier));

			continue;
		}

		let issue = resolved
			.get(identifier)
			.ok_or_else(|| eyre::eyre!("Resolved issue `{identifier}` disappeared from intake."))?;
		let facts = facts_by_identifier
			.get(identifier)
			.ok_or_else(|| eyre::eyre!("Issue facts for `{identifier}` disappeared."))?;
		let evaluation = evaluation_by_issue
			.get(identifier)
			.ok_or_else(|| eyre::eyre!("Issue evaluation for `{identifier}` disappeared."))?;

		rows.push(issue_batch::issue_report_row(issue, facts, evaluation, workflow));
	}

	let counts = issue_batch::classify_counts(&rows);

	if persist {
		state_store.upsert_execution_program(config.service_id(), program)?;
	}

	Ok(IssueBatchIntakeReport {
		service_id: config.service_id().to_owned(),
		program_id,
		dry_run,
		persisted: persist,
		counts,
		issues: rows,
	})
}
