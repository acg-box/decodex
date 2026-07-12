use std::collections::BTreeMap;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	config::ServiceConfig,
	execution_program::{ExecutionProgram, ExecutionWorkflowPolicy},
	lane_authority::{IntakeAuthority, IntakeAuthorityKind, ProjectBindingAttestation},
	prelude::{Result, eyre},
	program_intake::{
		IssueBatchIntakeReport,
		issue_batch::{identity, nodes, reporting},
		readiness,
	},
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

	let issue_identifiers = identity::normalize_issue_identifiers(issue_identifiers)?;
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
		identity::issue_batch_fingerprint(config.service_id(), &issue_identifiers, &resolved);
	let batch_identity_fingerprint =
		identity::issue_batch_identity_fingerprint(config.service_id(), &issue_identifiers);
	let program_id =
		identity::issue_batch_program_id(config.service_id(), &batch_identity_fingerprint);
	let supplied_node_ids = issue_identifiers
		.iter()
		.map(|identifier| (identifier.clone(), identity::node_id_for_issue(identifier)))
		.collect::<BTreeMap<_, _>>();
	let mut nodes = Vec::new();
	let mut dependency_snapshots = Vec::new();
	let mut facts_by_identifier = BTreeMap::new();

	for identifier in &issue_identifiers {
		if let Some(issue) = resolved.get(identifier) {
			let facts = nodes::issue_facts(tracker, workflow, issue, &active_label)?;

			dependency_snapshots
				.extend(nodes::dependency_snapshots_for(issue, &supplied_node_ids)?);
			nodes.push(nodes::issue_node(issue, &facts, workflow, &supplied_node_ids)?);
			facts_by_identifier.insert(identifier.clone(), facts);
		} else {
			nodes.push(nodes::unmapped_node(identifier)?);
		}
	}

	let program = ExecutionProgram::from_issue_batch_intake(
		&program_id,
		config.service_id(),
		&batch_fingerprint,
		format!("Issue-batch intake for {} issue(s).", issue_identifiers.len()),
		nodes,
	)?;
	let context = readiness::intake_readiness_context(
		config.service_id(),
		workflow,
		state_store,
		&program,
		dependency_snapshots,
	)?;
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
			rows.push(reporting::unmapped_report_row(identifier));

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

		rows.push(reporting::issue_report_row(issue, facts, evaluation, workflow));
	}

	let counts = reporting::classify_counts(&rows);

	if persist {
		let binding = match state_store.registered_project_binding(config.service_id())? {
			Some(binding) => binding,
			None => {
				#[cfg(not(test))]
				eyre::bail!("Project is not registered; issue-batch intake is forbidden.");
				#[cfg(test)]
				config.project_binding("test-config-fingerprint")
			},
		};
		for issue in resolved.values() {
			if issue.team.id != binding.tracker_team_id() {
				eyre::bail!(
					"Issue `{}` is outside the registered project binding.",
					issue.identifier
				);
			}
		}
		let plan = program.program_intake_plan().ok_or_else(|| {
			eyre::eyre!("Issue-batch Execution Program is missing its intake plan.")
		})?;
		let authority = if let Some(existing) =
			state_store.intake_authority_for_program(config.service_id(), program.program_id())?
		{
			existing
		} else {
			let now = OffsetDateTime::now_utc();
			let accepted_at = now.format(&Rfc3339)?;
			IntakeAuthority::new(
				&format!("intake-authority-{}", &batch_identity_fingerprint[..16]),
				config.service_id(),
				ProjectBindingAttestation::new(&binding),
				plan.plan_id(),
				program.program_id(),
				"local_operator",
				"issue_batch_intake",
				&format!("issue-batch:{batch_identity_fingerprint}"),
				&accepted_at,
				now.unix_timestamp(),
				IntakeAuthorityKind::IssueBatch {
					accepted_intake_id: program.program_id().to_owned(),
					batch_fingerprint: batch_fingerprint.clone(),
				},
			)?
		};
		let duplicate_program_ids =
			exact_issue_batch_duplicate_program_ids(state_store, config.service_id(), &program)?;

		state_store.upsert_execution_program_with_intake_authority(
			config.service_id(),
			program,
			authority,
		)?;

		for duplicate_program_id in duplicate_program_ids {
			state_store.delete_execution_program(config.service_id(), &duplicate_program_id)?;
		}
	}

	Ok(IssueBatchIntakeReport {
		service_id: config.service_id().to_owned(),
		program_id,
		dry_run,
		persisted: persist,
		scheduler_visible: persist,
		counts,
		issues: rows,
	})
}

fn exact_issue_batch_duplicate_program_ids(
	state_store: &StateStore,
	service_id: &str,
	replacement: &ExecutionProgram,
) -> Result<Vec<String>> {
	let replacement_node_ids =
		replacement.nodes().iter().map(|node| node.node_id()).collect::<Vec<_>>();
	let mut duplicates = Vec::new();

	for record in state_store.list_execution_programs(service_id)? {
		let program = record.program();
		let is_issue_batch = program
			.program_intake_plan()
			.is_some_and(|plan| plan.intake_kind().as_str() == "issue_batch_intake");
		let node_ids = program.nodes().iter().map(|node| node.node_id()).collect::<Vec<_>>();

		if is_issue_batch
			&& program.program_id() != replacement.program_id()
			&& node_ids == replacement_node_ids
		{
			duplicates.push(program.program_id().to_owned());
		}
	}

	Ok(duplicates)
}
