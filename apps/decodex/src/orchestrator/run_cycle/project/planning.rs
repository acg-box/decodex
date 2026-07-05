use crate::orchestrator::run_cycle::{
	self, IssueDispatchMode, IssueRunPlan, IssueTracker, PreferredRunIdentity,
	PrepareIssueRunContext, Result, RetryIssueStateHint, ServiceConfig, StateStore,
	WorkflowDocument, WorktreeManager, eyre, project::candidate, slice,
};

pub(crate) fn plan_project_issue_run_with_exclusions<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dry_run: bool,
	excluded_issue_ids: &[&str],
) -> Result<Option<IssueRunPlan>>
where
	T: IssueTracker,
{
	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());

	state_store.configure_dispatch_slot_root(project.service_id(), project.worktree_root())?;

	let recovered_state = run_cycle::recover_runtime_state_from_tracker_and_worktrees(
		tracker,
		project,
		workflow,
		state_store,
	)?;

	if !dry_run {
		run_cycle::reconcile_project_state(
			tracker,
			project,
			workflow,
			state_store,
			&worktree_manager,
		)?;
		run_cycle::reconcile_post_review_orchestration(tracker, project, workflow, state_store)?;
	}

	let Some(selected_issue) = candidate::select_project_issue_run_candidate(
		tracker,
		project,
		workflow,
		state_store,
		recovered_state,
		dry_run,
		excluded_issue_ids,
	)?
	else {
		return Ok(None);
	};
	let mut refreshed_issues = tracker.refresh_issues(slice::from_ref(&selected_issue.issue.id))?;
	let Some(issue) = refreshed_issues.pop() else {
		return Ok(None);
	};
	let dispatch_mode = selected_issue.dispatch_mode;
	let preferred_run_identity = selected_issue.preferred_run_identity;
	let program_dispatch = selected_issue.program_dispatch.clone();

	if !dry_run && dispatch_mode != IssueDispatchMode::Closeout {
		run_cycle::ensure_project_has_no_merged_worktree_cleanup_debt(project)?;
	}
	if !run_cycle::issue_passes_current_dispatch_policy(
		tracker,
		&issue,
		project,
		workflow,
		state_store,
		dispatch_mode,
		RetryIssueStateHint::default(),
	)? {
		if dispatch_mode == IssueDispatchMode::Closeout
			&& let Some(reason) = run_cycle::closeout_dispatch_block_reason(
				tracker,
				&issue,
				project,
				workflow,
				state_store,
			)? {
			if !dry_run {
				eyre::bail!("retained closeout dispatch blocked: {reason}");
			}

			return Ok(None);
		}

		return replan_project_issue_run_after_excluding(
			tracker,
			project,
			workflow,
			state_store,
			dry_run,
			excluded_issue_ids,
			issue.id.as_str(),
		);
	}

	let Some(issue_run) = run_cycle::prepare_issue_run(
		PrepareIssueRunContext {
			tracker,
			project,
			workflow,
			state_store,
			worktree_manager: &worktree_manager,
			dry_run,
			lease_preacquired: false,
			dispatch_mode,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_run_identity: preferred_run_identity.as_ref().map(|identity| {
				PreferredRunIdentity {
					run_id: identity.run_id.as_str(),
					attempt_number: identity.attempt_number,
				}
			}),
			preferred_retry_budget_base: None,
		},
		issue,
	)?
	else {
		return Ok(None);
	};

	if !dry_run && let Some(program_dispatch) = program_dispatch.as_ref() {
		run_cycle::record_program_dispatch_selected(
			state_store,
			project.service_id(),
			&issue_run,
			program_dispatch,
		)?;
	}

	Ok(Some(issue_run))
}

fn replan_project_issue_run_after_excluding<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dry_run: bool,
	excluded_issue_ids: &[&str],
	issue_id: &str,
) -> Result<Option<IssueRunPlan>>
where
	T: IssueTracker,
{
	let mut next_excluded_issue_ids = excluded_issue_ids.to_vec();

	next_excluded_issue_ids.push(issue_id);

	plan_project_issue_run_with_exclusions(
		tracker,
		project,
		workflow,
		state_store,
		dry_run,
		&next_excluded_issue_ids,
	)
}
