use std::collections::{BTreeMap, BTreeSet};

use crate::{
	execution_program::{ExecutionConflictDomain, ExecutionDependencySnapshot},
	orchestrator::{
		self, ExecutionProgramReadinessContext, ProgramIssueSnapshot, RefreshedExecutionProgram,
		Result, StateStore, WorkflowDocument,
	},
};

pub(crate) fn execution_program_readiness_context(
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	programs: &[RefreshedExecutionProgram],
) -> Result<ExecutionProgramReadinessContext> {
	let dependency_snapshots = execution_program_dependency_snapshots(programs)?;
	let occupied_conflict_domains =
		execution_program_occupied_conflict_domains(service_id, workflow, state_store, programs)?;
	let active_issue_ids =
		execution_program_active_issue_ids(service_id, workflow, state_store, programs)?;

	Ok(ExecutionProgramReadinessContext::new()
		.with_dependency_snapshots(dependency_snapshots)
		.with_occupied_conflict_domains(occupied_conflict_domains)
		.with_active_issue_ids(active_issue_ids))
}

pub(crate) fn execution_program_dependency_snapshots(
	programs: &[RefreshedExecutionProgram],
) -> Result<Vec<ExecutionDependencySnapshot>> {
	let mut snapshots = BTreeMap::new();

	for refreshed in programs {
		for node in refreshed.program.nodes() {
			let Some(snapshot) = refreshed.issues_by_node.get(node.node_id()) else {
				continue;
			};

			insert_dependency_snapshot(&mut snapshots, node.node_id(), &snapshot.issue.state.name)?;
			insert_dependency_snapshot(
				&mut snapshots,
				&snapshot.issue.identifier,
				&snapshot.issue.state.name,
			)?;

			for blocker in &snapshot.issue.blockers {
				insert_dependency_snapshot(
					&mut snapshots,
					&blocker.identifier,
					&blocker.state.name,
				)?;
			}
		}
	}

	Ok(snapshots.into_values().collect())
}

pub(crate) fn insert_dependency_snapshot(
	snapshots: &mut BTreeMap<String, ExecutionDependencySnapshot>,
	dependency_id: &str,
	state: &str,
) -> Result<()> {
	if snapshots.contains_key(dependency_id) {
		return Ok(());
	}

	snapshots.insert(
		dependency_id.to_owned(),
		ExecutionDependencySnapshot::tracker_state(dependency_id, state)?,
	);

	Ok(())
}

pub(crate) fn execution_program_occupied_conflict_domains(
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	programs: &[RefreshedExecutionProgram],
) -> Result<Vec<ExecutionConflictDomain>> {
	let retained_issue_ids = state_store
		.list_worktrees(service_id)?
		.into_iter()
		.map(|worktree| worktree.issue_id().to_owned())
		.collect::<BTreeSet<_>>();
	let mut occupied = Vec::new();
	let mut seen = BTreeSet::new();

	for refreshed in programs {
		for node in refreshed.program.nodes() {
			let Some(snapshot) = refreshed.issues_by_node.get(node.node_id()) else {
				continue;
			};

			if !program_issue_occupies_conflict_domain(
				service_id,
				workflow,
				state_store,
				&retained_issue_ids,
				snapshot,
			)? {
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

pub(crate) fn program_issue_occupies_conflict_domain(
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	retained_issue_ids: &BTreeSet<String>,
	snapshot: &ProgramIssueSnapshot,
) -> Result<bool> {
	let issue = &snapshot.issue;
	let retained_nonterminal = retained_issue_ids.contains(&issue.id)
		&& !orchestrator::state_name_is_terminal(&issue.state.name, workflow);

	Ok(snapshot.has_active_label
		|| snapshot.has_needs_attention_label
		|| snapshot.has_post_review_lifecycle
		|| retained_nonterminal
		|| state_store.issue_has_active_shared_claim(service_id, &issue.id)?)
}

fn execution_program_active_issue_ids(
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	programs: &[RefreshedExecutionProgram],
) -> Result<Vec<String>> {
	let retained_issue_ids = state_store
		.list_worktrees(service_id)?
		.into_iter()
		.map(|worktree| worktree.issue_id().to_owned())
		.collect::<BTreeSet<_>>();
	let mut active_issue_ids = state_store
		.list_active_shared_leases(service_id)?
		.into_iter()
		.map(|lease| lease.issue_id().to_owned())
		.collect::<BTreeSet<_>>();

	for refreshed in programs {
		for snapshot in refreshed.issues_by_node.values() {
			let issue = &snapshot.issue;

			if retained_issue_ids.contains(&issue.id)
				&& !orchestrator::state_name_is_terminal(&issue.state.name, workflow)
			{
				active_issue_ids.insert(issue.id.clone());
			}
		}
	}

	Ok(active_issue_ids.into_iter().collect())
}
