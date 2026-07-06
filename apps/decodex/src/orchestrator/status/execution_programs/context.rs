use std::collections::{BTreeMap, BTreeSet};

use crate::{
	execution_program::{
		ExecutionConflictDomain, ExecutionDependencySnapshot, ExecutionProgramReadinessContext,
	},
	orchestrator,
	prelude::Result,
	state::{ExecutionProgramRecord, StateStore},
	workflow::WorkflowDocument,
};

pub(crate) fn operator_execution_program_readiness_context(
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	records: &[ExecutionProgramRecord],
) -> Result<ExecutionProgramReadinessContext> {
	let dependency_snapshots = self::operator_execution_program_dependency_snapshots(records)?;
	let occupied_conflict_domains = self::operator_execution_program_occupied_conflict_domains(
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

	Ok(ExecutionProgramReadinessContext::new()
		.with_dependency_snapshots(dependency_snapshots)
		.with_occupied_conflict_domains(occupied_conflict_domains)
		.with_active_issue_ids(active_issue_ids))
}

pub(crate) fn operator_execution_program_mapped_issue_ids(
	records: &[ExecutionProgramRecord],
) -> Vec<String> {
	records
		.iter()
		.flat_map(|record| record.program().nodes())
		.filter_map(|node| node.linear_issue().map(|issue| issue.issue_id().to_owned()))
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect()
}

fn operator_execution_program_dependency_snapshots(
	records: &[ExecutionProgramRecord],
) -> Result<Vec<ExecutionDependencySnapshot>> {
	let mut snapshots = BTreeMap::new();

	for record in records {
		for node in record.program().nodes() {
			let Some(issue) = node.linear_issue() else {
				continue;
			};

			orchestrator::insert_dependency_snapshot(
				&mut snapshots,
				node.node_id(),
				issue.issue_state(),
			)?;
			orchestrator::insert_dependency_snapshot(
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
) -> Result<Vec<ExecutionConflictDomain>> {
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
				&& !orchestrator::state_name_is_terminal(issue.issue_state(), workflow);
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
