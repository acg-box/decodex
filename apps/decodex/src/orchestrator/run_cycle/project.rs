#[allow(clippy::wildcard_imports)] use super::*;

pub(crate) fn run_project_once<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dry_run: bool,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	run_project_once_with_exclusions(tracker, project, workflow, state_store, dry_run, &[])
}

fn run_project_once_with_exclusions<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dry_run: bool,
	excluded_issue_ids: &[&str],
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	let Some(issue_run) = plan_project_issue_run_with_exclusions(
		tracker,
		project,
		workflow,
		state_store,
		dry_run,
		excluded_issue_ids,
	)?
	else {
		if !dry_run {
			reconcile_terminal_thread_archive_backlog_best_effort(project, workflow, state_store);
		}

		return Ok(None);
	};

	complete_issue_run(tracker, project, workflow, state_store, issue_run, dry_run)
}

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

	let recovered_state =
		recover_runtime_state_from_tracker_and_worktrees(tracker, project, workflow, state_store)?;

	if !dry_run {
		reconcile_project_state(tracker, project, workflow, state_store, &worktree_manager)?;
		reconcile_post_review_orchestration(tracker, project, workflow, state_store)?;
	}

	let Some(selected_issue) = select_project_issue_run_candidate(
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
		ensure_project_has_no_merged_worktree_cleanup_debt(project)?;
	}
	if !dispatch_mode.allows_issue(
		tracker,
		&issue,
		project,
		workflow,
		state_store,
		RetryIssueStateHint::default(),
	)? {
		if dispatch_mode == IssueDispatchMode::Closeout
			&& let Some(reason) =
				closeout_dispatch_block_reason(tracker, &issue, project, workflow, state_store)?
		{
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

	let Some(issue_run) = prepare_issue_run(
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
		record_program_dispatch_selected(
			state_store,
			project.service_id(),
			&issue_run,
			program_dispatch,
		)?;
	}

	Ok(Some(issue_run))
}

fn select_project_issue_run_candidate<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	recovered_state: RecoveredRuntimeState,
	dry_run: bool,
	excluded_issue_ids: &[&str],
) -> Result<Option<SelectedIssueRunCandidate>>
where
	T: IssueTracker,
{
	let selected_retry_issue = select_recovered_retry_issue_candidate(
		project,
		state_store,
		recovered_state,
		excluded_issue_ids,
	)?;
	let selected_post_review_issue = select_post_review_issue_candidate(
		tracker,
		project,
		workflow,
		state_store,
		excluded_issue_ids,
	)?;

	if let Some(candidate) = selected_retry_issue.or(selected_post_review_issue) {
		return Ok(Some(candidate));
	}
	if let Some(candidate) = select_execution_program_run_candidate(
		tracker,
		project,
		workflow,
		state_store,
		excluded_issue_ids,
	)? {
		return Ok(Some(candidate));
	}

	let issues = queued_issues_for_dispatch(tracker, project, workflow, dry_run)?;

	Ok(select_issue_candidate_with_exclusions(
		tracker,
		issues,
		workflow,
		state_store,
		project.service_id(),
		excluded_issue_ids,
	)?
	.map(|issue| SelectedIssueRunCandidate::new(issue, IssueDispatchMode::Normal)))
}

fn select_recovered_retry_issue_candidate(
	project: &ServiceConfig,
	state_store: &StateStore,
	recovered_state: RecoveredRuntimeState,
	excluded_issue_ids: &[&str],
) -> Result<Option<SelectedIssueRunCandidate>> {
	for issue in recovered_state.recoverable_issues {
		if excluded_issue_ids.contains(&issue.id.as_str()) {
			continue;
		}
		if state_store.issue_has_active_shared_claim(project.service_id(), &issue.id)? {
			continue;
		}

		return Ok(Some(SelectedIssueRunCandidate::new(issue, IssueDispatchMode::Retry)));
	}

	Ok(None)
}
fn queued_issues_for_dispatch<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	dry_run: bool,
) -> Result<Vec<TrackerIssue>>
where
	T: IssueTracker,
{
	let queue_label = tracker::automation_queue_label(project.service_id());

	clear_terminal_queued_lane_labels(
		tracker,
		project,
		workflow,
		tracker.list_issues_with_label(&queue_label)?,
		dry_run,
	)
}

fn clear_terminal_queued_lane_labels<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	issues: Vec<TrackerIssue>,
	dry_run: bool,
) -> Result<Vec<TrackerIssue>>
where
	T: IssueTracker,
{
	let mut nonterminal_issues = Vec::with_capacity(issues.len());

	for issue in issues {
		if is_terminal_issue(&issue, workflow) {
			if !dry_run {
				tracker::clear_automation_lane_labels(tracker, &issue, project.service_id())?;

				tracing::info!(
					project_id = project.service_id(),
					issue_id = issue.id,
					issue = issue.identifier,
					"Cleared automation lane labels from terminal queued issue."
				);
			}

			continue;
		}

		nonterminal_issues.push(issue);
	}

	Ok(nonterminal_issues)
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
