//! Execution Program readback for operator status snapshots.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
	config::ServiceConfig,
	execution_program::{ExecutionConflictDomain, ExecutionDependencySnapshot},
	state::{ExecutionProgramRecord, StateStore},
	tracker::IssueTracker,
	workflow::WorkflowDocument,
};

use super::{
	OperatorExecutionProgramReadback, OperatorExecutionProgramStatus,
	execution_program_readiness_context, insert_dependency_snapshot,
	refresh_execution_program_issues, refresh_execution_program_local_lifecycle_facts,
	refresh_execution_program_tracker_facts, state_name_is_terminal,
};

pub(super) fn operator_execution_program_statuses<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> crate::prelude::Result<OperatorExecutionProgramReadback>
where
	T: IssueTracker + ?Sized,
{
	let records = state_store.list_execution_programs(project.service_id())?;

	if records.is_empty() {
		return Ok(OperatorExecutionProgramReadback {
			statuses: Vec::new(),
			issue_metadata_unavailable: false,
		});
	}

	match operator_execution_program_statuses_with_live_tracker(
		tracker,
		project,
		workflow,
		state_store,
		&records,
	) {
		Ok(statuses) => {
			Ok(OperatorExecutionProgramReadback { statuses, issue_metadata_unavailable: false })
		},
		Err(error) => {
			let _ = error;

			tracing::warn!(
				project_id = project.service_id(),
				"Skipped live tracker metadata hydration for Execution Program status; sensitive tracker details were withheld."
			);

			Ok(OperatorExecutionProgramReadback {
				statuses: operator_execution_program_statuses_from_persisted(
					project,
					workflow,
					state_store,
					&records,
				)?,
				issue_metadata_unavailable: true,
			})
		},
	}
}

fn operator_execution_program_statuses_with_live_tracker<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	records: &[ExecutionProgramRecord],
) -> crate::prelude::Result<Vec<OperatorExecutionProgramStatus>>
where
	T: IssueTracker + ?Sized,
{
	let policy = crate::execution_program::ExecutionWorkflowPolicy::from_workflow(
		project.service_id(),
		workflow,
	)?;
	let mapped_issue_ids = operator_execution_program_mapped_issue_ids(records);
	let refreshed_issues = refresh_execution_program_issues(tracker, records)?;

	if mapped_issue_ids.iter().any(|issue_id| !refreshed_issues.contains_key(issue_id)) {
		crate::prelude::eyre::bail!("Execution Program tracker metadata was incomplete.");
	}

	let refreshed_programs = records
		.iter()
		.cloned()
		.map(|record| {
			refresh_execution_program_tracker_facts(
				tracker,
				state_store,
				project.service_id(),
				workflow,
				record,
				&refreshed_issues,
			)
		})
		.collect::<crate::prelude::Result<Vec<_>>>()?;
	let context = execution_program_readiness_context(
		project.service_id(),
		workflow,
		state_store,
		&refreshed_programs,
	)?;
	let mut statuses = Vec::new();

	for refreshed in refreshed_programs {
		let record = &refreshed.record;
		let program = &refreshed.program;
		let evaluation = if let Some(source_contract_id) = record.source_contract_id() {
			let Some(contract) =
				state_store.decision_contract(project.service_id(), source_contract_id)?
			else {
				statuses.push(OperatorExecutionProgramStatus::missing_contract(record));

				continue;
			};

			program.evaluate(contract.contract(), &policy, &context)?
		} else {
			program.evaluate_issue_batch(&policy, &context)?
		};

		if program != record.program() {
			state_store.upsert_execution_program(project.service_id(), (*program).clone())?;
		}

		statuses.push(OperatorExecutionProgramStatus::from_summary(
			record,
			evaluation.operator_summary(),
			&evaluation,
		));
	}

	statuses.sort_by(|left, right| left.program_id.cmp(&right.program_id));

	Ok(statuses)
}

fn operator_execution_program_mapped_issue_ids(records: &[ExecutionProgramRecord]) -> Vec<String> {
	records
		.iter()
		.flat_map(|record| record.program().nodes())
		.filter_map(|node| node.linear_issue().map(|issue| issue.issue_id().to_owned()))
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect()
}

fn operator_execution_program_statuses_from_persisted(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	records: &[ExecutionProgramRecord],
) -> crate::prelude::Result<Vec<OperatorExecutionProgramStatus>> {
	let policy = crate::execution_program::ExecutionWorkflowPolicy::from_workflow(
		project.service_id(),
		workflow,
	)?;
	let context = operator_execution_program_readiness_context(
		project.service_id(),
		workflow,
		state_store,
		records,
	)?;
	let mut statuses = Vec::new();

	for record in records {
		let mut nodes = Vec::with_capacity(record.program().nodes().len());

		for node in record.program().nodes() {
			nodes.push(refresh_execution_program_local_lifecycle_facts(
				state_store,
				project.service_id(),
				node,
			)?);
		}

		let program = record.program().clone().with_nodes(nodes)?;
		let evaluation = if let Some(source_contract_id) = record.source_contract_id() {
			let Some(contract) =
				state_store.decision_contract(project.service_id(), source_contract_id)?
			else {
				statuses.push(OperatorExecutionProgramStatus::missing_contract(record));

				continue;
			};

			program.evaluate(contract.contract(), &policy, &context)?
		} else {
			program.evaluate_issue_batch(&policy, &context)?
		};

		statuses.push(OperatorExecutionProgramStatus::from_summary(
			record,
			evaluation.operator_summary(),
			&evaluation,
		));
	}

	statuses.sort_by(|left, right| left.program_id.cmp(&right.program_id));

	Ok(statuses)
}

fn operator_execution_program_readiness_context(
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	records: &[ExecutionProgramRecord],
) -> crate::prelude::Result<crate::execution_program::ExecutionProgramReadinessContext> {
	let dependency_snapshots = operator_execution_program_dependency_snapshots(records)?;
	let occupied_conflict_domains = operator_execution_program_occupied_conflict_domains(
		service_id,
		workflow,
		state_store,
		records,
	)?;
	let active_issue_ids = state_store
		.list_active_shared_leases(service_id)?
		.into_iter()
		.map(|lease| lease.issue_id().to_owned())
		.collect::<Vec<_>>();

	Ok(crate::execution_program::ExecutionProgramReadinessContext::new()
		.with_dependency_snapshots(dependency_snapshots)
		.with_occupied_conflict_domains(occupied_conflict_domains)
		.with_active_issue_ids(active_issue_ids))
}

fn operator_execution_program_dependency_snapshots(
	records: &[ExecutionProgramRecord],
) -> crate::prelude::Result<Vec<ExecutionDependencySnapshot>> {
	let mut snapshots = BTreeMap::new();

	for record in records {
		for node in record.program().nodes() {
			let Some(issue) = node.linear_issue() else {
				continue;
			};

			insert_dependency_snapshot(&mut snapshots, node.node_id(), issue.issue_state())?;
			insert_dependency_snapshot(
				&mut snapshots,
				issue.issue_identifier(),
				issue.issue_state(),
			)?;
		}
	}

	Ok(snapshots.into_values().collect())
}

fn operator_execution_program_occupied_conflict_domains(
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	records: &[ExecutionProgramRecord],
) -> crate::prelude::Result<Vec<ExecutionConflictDomain>> {
	let retained_issue_ids = state_store
		.list_worktrees(service_id)?
		.into_iter()
		.map(|worktree| worktree.issue_id().to_owned())
		.collect::<BTreeSet<_>>();
	let mut occupied = Vec::new();
	let mut seen = BTreeSet::new();

	for record in records {
		for node in record.program().nodes() {
			let Some(issue) = node.linear_issue() else {
				continue;
			};
			let retained_nonterminal = retained_issue_ids.contains(issue.issue_id())
				&& !state_name_is_terminal(issue.issue_state(), workflow);
			let has_post_review_lifecycle =
				state_store.issue_has_review_lifecycle_record(service_id, issue.issue_id())?;
			let issue_occupies_domain = issue.has_active_label()
				|| issue.has_needs_attention_label()
				|| has_post_review_lifecycle
				|| retained_nonterminal
				|| state_store.issue_has_active_shared_claim(service_id, issue.issue_id())?;

			if !issue_occupies_domain {
				continue;
			}

			for domain in node.conflict_domains() {
				let key = format!("{}:{}", domain.kind().as_str(), domain.key());

				if seen.insert(key) {
					occupied.push(domain.clone());
				}
			}
		}
	}

	Ok(occupied)
}
