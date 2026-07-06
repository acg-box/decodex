use std::collections::BTreeSet;

use crate::{
	execution_program::{
		ExecutionConflictDomain, ExecutionDependencySnapshot, ExecutionProgram,
		ExecutionProgramReadinessContext,
	},
	orchestrator,
	prelude::Result,
	state::StateStore,
	workflow::WorkflowDocument,
};

pub(in crate::program_intake) fn intake_readiness_context(
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	program: &ExecutionProgram,
	dependency_snapshots: Vec<ExecutionDependencySnapshot>,
) -> Result<ExecutionProgramReadinessContext> {
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

	for node in program.nodes() {
		let Some(issue) = node.linear_issue() else {
			continue;
		};

		if retained_issue_ids.contains(issue.issue_id())
			&& !orchestrator::state_name_is_terminal(issue.issue_state(), workflow)
		{
			active_issue_ids.insert(issue.issue_id().to_owned());
		}
	}

	let occupied_conflict_domains =
		intake_occupied_conflict_domains(service_id, workflow, state_store)?;

	Ok(ExecutionProgramReadinessContext::new()
		.with_dependency_snapshots(dependency_snapshots)
		.with_occupied_conflict_domains(occupied_conflict_domains)
		.with_active_issue_ids(active_issue_ids))
}

fn intake_occupied_conflict_domains(
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<Vec<ExecutionConflictDomain>> {
	let retained_issue_ids = state_store
		.list_worktrees(service_id)?
		.into_iter()
		.map(|worktree| worktree.issue_id().to_owned())
		.collect::<BTreeSet<_>>();
	let mut occupied = Vec::new();
	let mut seen = BTreeSet::new();

	for record in state_store.list_execution_programs(service_id)? {
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
