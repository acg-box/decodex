use std::collections::BTreeSet;

use crate::orchestrator::{
	self, IssueTracker, ProgramSchedulerSelection, Result, RunSummary, SelectedIssueRunCandidate,
	StateStore, TargetIssueRunContext, WorktreeManager, run_cycle::target_issue,
};

pub(crate) fn run_target_status_visible_program_once<T>(
	context: TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	let Some(_) = select_target_status_visible_program_candidate(&context)? else {
		return Ok(None);
	};

	target_issue::run_target_issue_once(context)
}

pub(crate) fn select_target_status_visible_program_candidate<T>(
	context: &TargetIssueRunContext<'_, T>,
) -> Result<Option<SelectedIssueRunCandidate>>
where
	T: IssueTracker,
{
	let worktree_manager = WorktreeManager::new(
		context.project.service_id(),
		context.project.repo_root(),
		context.project.worktree_root(),
	);

	context.state_store.configure_dispatch_slot_root(
		context.project.service_id(),
		context.project.worktree_root(),
	)?;

	let target_issue_id = target_issue::resolve_target_issue_id(context.tracker, context.issue_id)?;

	if !context.lease_preacquired {
		orchestrator::recover_runtime_state_from_tracker_and_worktrees(
			context.tracker,
			context.project,
			context.workflow,
			context.state_store,
		)?;

		if !context.dry_run {
			orchestrator::reconcile_project_state(
				context.tracker,
				context.project,
				context.workflow,
				context.state_store,
				&worktree_manager,
			)?;
			orchestrator::reconcile_post_review_orchestration(
				context.tracker,
				context.project,
				context.workflow,
				context.state_store,
			)?;
		}
	}

	let excluded_issue_ids = execution_program_non_target_mapped_issue_ids(
		context.state_store,
		context.project.service_id(),
		&target_issue_id,
	)?;
	let excluded_issue_ids = excluded_issue_ids.iter().map(String::as_str).collect::<Vec<_>>();
	let ProgramSchedulerSelection { selected, .. } =
		orchestrator::select_execution_program_run_candidate_with_summary(
			context.tracker,
			context.project,
			context.workflow,
			context.state_store,
			&excluded_issue_ids,
		)?;
	let Some(selected) = selected else {
		return Ok(None);
	};

	if selected.issue.id != target_issue_id {
		return Ok(None);
	}

	Ok(Some(selected))
}

fn execution_program_non_target_mapped_issue_ids(
	state_store: &StateStore,
	service_id: &str,
	target_issue_id: &str,
) -> Result<Vec<String>> {
	let records = state_store.list_execution_programs(service_id)?;

	Ok(records
		.iter()
		.flat_map(|record| record.program().nodes())
		.filter_map(|node| node.linear_issue())
		.map(|issue| issue.issue_id())
		.filter(|issue_id| *issue_id != target_issue_id)
		.map(str::to_owned)
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect())
}
