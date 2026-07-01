#[allow(clippy::wildcard_imports)] use super::*;

use state::PreacquiredLeaseGuards;

use crate::commit_message;

const INTERNAL_RETAINED_DRAIN_MAX_PASSES: usize = 2;

pub(crate) fn run_configured_cycle(request: RunCycleRequest<'_>) -> Result<Option<RunSummary>> {
	let config = ServiceConfig::from_path(request.config_path)?;
	let workflow = load_configured_cycle_workflow(&config, request.preferred_workflow_snapshot)?;
	let api_key = config.tracker().resolve_api_key()?;
	let tracker = LinearClient::new(api_key)?;

	if let Some(issue_id) = request.preferred_issue_id {
		let target_context = TargetIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: request.state_store,
			issue_id,
			preferred_issue_state: request.preferred_issue_state,
			preferred_initial_issue_state: request.preferred_initial_issue_state,
			dry_run: request.dry_run,
			lease_preacquired: request.preferred_lease_acquired,
			preferred_issue_claim_fd: request.preferred_issue_claim_fd,
			preferred_dispatch_slot_fd: request.preferred_dispatch_slot_fd,
			preferred_dispatch_slot_index: request.preferred_dispatch_slot_index,
			dispatch_mode: request.preferred_dispatch_mode.unwrap_or(IssueDispatchMode::Normal),
			preferred_run_identity: request.preferred_run_identity,
			preferred_retry_budget_base: request.preferred_retry_budget_base,
		};

		return match request.preferred_dispatch_mode {
			Some(_) => run_target_issue_once(target_context),
			None => run_target_issue_once_with_inferred_dispatch(target_context),
		};
	}

	run_project_once(&tracker, &config, &workflow, request.state_store, request.dry_run)
}

pub(crate) fn load_configured_cycle_workflow(
	config: &ServiceConfig,
	preferred_workflow_snapshot: Option<&str>,
) -> Result<WorkflowDocument> {
	let workflow_path = config.workflow_path().to_path_buf();

	match preferred_workflow_snapshot {
		Some(snapshot) => WorkflowDocument::parse_markdown(snapshot),
		None => WorkflowDocument::from_path(&workflow_path),
	}
}

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

pub(crate) fn run_target_issue_once<T>(
	context: TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
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

	let issue_id = resolve_target_issue_id(context.tracker, context.issue_id)?;

	if !context.dry_run {
		context.state_store.canonicalize_issue_identity(context.issue_id, &issue_id)?;
	}
	if context.lease_preacquired && !context.dry_run {
		adopt_preacquired_target_issue_lease(&context, &issue_id)?;
	}
	if !context.lease_preacquired {
		recover_runtime_state_from_tracker_and_worktrees(
			context.tracker,
			context.project,
			context.workflow,
			context.state_store,
		)?;

		if !context.dry_run {
			reconcile_project_state(
				context.tracker,
				context.project,
				context.workflow,
				context.state_store,
				&worktree_manager,
			)?;
		}
	}

	let Some(issue) = refresh_issue(context.tracker, &issue_id)? else {
		return Ok(None);
	};
	let closeout_preferred_run_identity = target_closeout_preferred_run_identity(&context, &issue)?;
	let preferred_run_identity = preferred_run_identity_with_closeout_fallback(
		context.preferred_run_identity,
		closeout_preferred_run_identity.as_ref(),
	);
	let retry_state_hint = RetryIssueStateHint {
		preferred_issue_state: context.preferred_issue_state,
		preferred_initial_issue_state: context.preferred_initial_issue_state,
	};

	if !context.dispatch_mode.allows_issue(
		context.tracker,
		&issue,
		context.project,
		context.workflow,
		context.state_store,
		retry_state_hint,
	)? {
		ensure_target_closeout_dispatch_is_unblocked(&context, &issue)?;

		return Ok(None);
	}

	let reuses_existing_closeout_claim =
		target_issue_reuses_existing_closeout_claim(&context, &issue_id, &issue)?;

	if target_issue_active_claim_blocks_dispatch(&context, &issue_id, &issue)? {
		return Ok(None);
	}
	if !context.dry_run && context.dispatch_mode != IssueDispatchMode::Closeout {
		ensure_project_has_no_merged_worktree_cleanup_debt(context.project)?;
	}

	let Some(issue_run) = prepare_issue_run(
		PrepareIssueRunContext {
			tracker: context.tracker,
			project: context.project,
			workflow: context.workflow,
			state_store: context.state_store,
			worktree_manager: &worktree_manager,
			dry_run: context.dry_run,
			lease_preacquired: context.lease_preacquired || reuses_existing_closeout_claim,
			dispatch_mode: context.dispatch_mode,
			preferred_issue_state: context.preferred_issue_state,
			preferred_initial_issue_state: context.preferred_initial_issue_state,
			preferred_run_identity,
			preferred_retry_budget_base: context.preferred_retry_budget_base,
		},
		issue,
	)?
	else {
		return Ok(None);
	};

	complete_issue_run(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		issue_run,
		context.dry_run,
	)
}

fn ensure_target_closeout_dispatch_is_unblocked<T>(
	context: &TargetIssueRunContext<'_, T>,
	issue: &TrackerIssue,
) -> Result<()>
where
	T: IssueTracker,
{
	if context.dry_run || context.dispatch_mode != IssueDispatchMode::Closeout {
		return Ok(());
	}

	let Some(reason) = closeout_dispatch_block_reason(
		context.tracker,
		issue,
		context.project,
		context.workflow,
		context.state_store,
	)?
	else {
		return Ok(());
	};

	eyre::bail!("retained closeout dispatch blocked: {reason}");
}

fn target_closeout_preferred_run_identity<T>(
	context: &TargetIssueRunContext<'_, T>,
	issue: &TrackerIssue,
) -> Result<Option<RetainedReviewRunIdentity>>
where
	T: IssueTracker,
{
	if context.dispatch_mode != IssueDispatchMode::Closeout
		|| context.preferred_run_identity.is_some()
	{
		return Ok(None);
	}

	retained_closeout_preferred_run_identity(
		context.state_store,
		context.project.service_id(),
		issue,
	)
}

fn preferred_run_identity_with_closeout_fallback<'a>(
	preferred_run_identity: Option<PreferredRunIdentity<'a>>,
	closeout_preferred_run_identity: Option<&'a RetainedReviewRunIdentity>,
) -> Option<PreferredRunIdentity<'a>> {
	match (preferred_run_identity, closeout_preferred_run_identity) {
		(Some(identity), _) => Some(identity),
		(None, Some(identity)) => Some(PreferredRunIdentity {
			run_id: identity.run_id.as_str(),
			attempt_number: identity.attempt_number,
		}),
		(None, None) => None,
	}
}

pub(crate) fn run_target_issue_once_with_inferred_dispatch<T>(
	context: TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	if target_issue_has_status_visible_review_repair(&context)? {
		return run_target_status_visible_review_repair_once(context);
	}
	if target_issue_has_status_visible_closeout(&context)? {
		return run_target_status_visible_closeout_once(context);
	}

	if let Some(summary) = run_target_status_visible_program_once(
		target_issue_run_context_with_dispatch_mode(&context, IssueDispatchMode::Program),
	)? {
		return Ok(Some(summary));
	}
	if let Some(summary) = run_target_issue_once(target_issue_run_context_with_dispatch_mode(
		&context,
		IssueDispatchMode::Normal,
	))? {
		return Ok(Some(summary));
	}
	if let Some(summary) = run_target_issue_once(target_issue_run_context_with_dispatch_mode(
		&context,
		IssueDispatchMode::Retry,
	))? {
		return Ok(Some(summary));
	}
	if let Some(summary) = run_target_status_visible_review_repair_once(
		target_issue_run_context_with_dispatch_mode(&context, IssueDispatchMode::ReviewRepair),
	)? {
		return Ok(Some(summary));
	}

	run_target_status_visible_closeout_once(context)
}

fn run_target_status_visible_program_once<T>(
	context: TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	let Some(_) = select_target_status_visible_program_candidate(&context)? else {
		return Ok(None);
	};

	run_target_issue_once(context)
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

	let target_issue_id = resolve_target_issue_id(context.tracker, context.issue_id)?;

	if !context.lease_preacquired {
		recover_runtime_state_from_tracker_and_worktrees(
			context.tracker,
			context.project,
			context.workflow,
			context.state_store,
		)?;

		if !context.dry_run {
			reconcile_project_state(
				context.tracker,
				context.project,
				context.workflow,
				context.state_store,
				&worktree_manager,
			)?;
			reconcile_post_review_orchestration(
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
		select_execution_program_run_candidate_with_summary(
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

fn target_issue_has_status_visible_review_repair<T>(
	context: &TargetIssueRunContext<'_, T>,
) -> Result<bool>
where
	T: IssueTracker,
{
	let target_issue_id = resolve_target_issue_id(context.tracker, context.issue_id)?;
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(context.project.github().token_env_var().to_owned()),
		github_command_path: context.project.github().command_path().map(Path::to_path_buf),
	};

	Ok(build_post_review_lane_statuses(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		&review_state_inspector,
	)?
	.into_iter()
	.any(|lane| lane.issue_id == target_issue_id && post_review_lane_is_repair_candidate(&lane)))
}

fn run_target_status_visible_review_repair_once<T>(
	context: TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	let target_issue_id = resolve_target_issue_id(context.tracker, context.issue_id)?;
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(context.project.github().token_env_var().to_owned()),
		github_command_path: context.project.github().command_path().map(Path::to_path_buf),
	};
	let Some(_issue) = select_target_post_review_repair_issue_candidate_with_inspector(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		&target_issue_id,
		context.issue_id,
		&review_state_inspector,
	)?
	else {
		return Ok(None);
	};

	run_target_issue_once(target_issue_run_context_with_dispatch_mode(
		&context,
		IssueDispatchMode::ReviewRepair,
	))
}

fn target_issue_has_status_visible_closeout<T>(
	context: &TargetIssueRunContext<'_, T>,
) -> Result<bool>
where
	T: IssueTracker,
{
	let target_issue_id = resolve_target_issue_id(context.tracker, context.issue_id)?;
	let completed_state = context.workflow.frontmatter().tracker().resolved_completed_state();
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(context.project.github().token_env_var().to_owned()),
		github_command_path: context.project.github().command_path().map(Path::to_path_buf),
	};

	Ok(build_post_review_lane_statuses(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		&review_state_inspector,
	)?
	.into_iter()
	.any(|lane| {
		lane.issue_id == target_issue_id
			&& post_review_lane_is_closeout_candidate(&lane, completed_state)
	}))
}

fn run_target_status_visible_closeout_once<T>(
	context: TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	let target_issue_id = resolve_target_issue_id(context.tracker, context.issue_id)?;
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(context.project.github().token_env_var().to_owned()),
		github_command_path: context.project.github().command_path().map(Path::to_path_buf),
	};
	let Some(candidate) = select_target_post_review_closeout_issue_candidate_with_inspector(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		&target_issue_id,
		context.issue_id,
		&review_state_inspector,
	)?
	else {
		return Ok(None);
	};
	let preferred_run_identity =
		candidate.preferred_run_identity.as_ref().map(|identity| PreferredRunIdentity {
			run_id: identity.run_id.as_str(),
			attempt_number: identity.attempt_number,
		});

	run_target_issue_once(TargetIssueRunContext {
		tracker: context.tracker,
		project: context.project,
		workflow: context.workflow,
		state_store: context.state_store,
		issue_id: context.issue_id,
		preferred_issue_state: context.preferred_issue_state,
		preferred_initial_issue_state: context.preferred_initial_issue_state,
		dry_run: context.dry_run,
		lease_preacquired: context.lease_preacquired,
		preferred_issue_claim_fd: context.preferred_issue_claim_fd,
		preferred_dispatch_slot_fd: context.preferred_dispatch_slot_fd,
		preferred_dispatch_slot_index: context.preferred_dispatch_slot_index,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity,
		preferred_retry_budget_base: context.preferred_retry_budget_base,
	})
}

fn target_issue_reuses_existing_closeout_claim<T>(
	context: &TargetIssueRunContext<'_, T>,
	issue_id: &str,
	issue: &TrackerIssue,
) -> Result<bool>
where
	T: IssueTracker,
{
	if context.lease_preacquired || context.dispatch_mode != IssueDispatchMode::Closeout {
		return Ok(false);
	}
	if !context.state_store.issue_has_active_shared_claim(context.project.service_id(), issue_id)? {
		return Ok(false);
	}
	if context.state_store.lease_for_issue(&issue.id)?.is_none() {
		return Ok(false);
	}

	Ok(!closeout_lane_active_claim_blocks_dispatch(context.project, context.state_store, issue)?)
}

pub(crate) fn target_issue_active_claim_blocks_dispatch<T>(
	context: &TargetIssueRunContext<'_, T>,
	issue_id: &str,
	issue: &TrackerIssue,
) -> Result<bool>
where
	T: IssueTracker,
{
	if context.lease_preacquired {
		return Ok(false);
	}
	if !context.state_store.issue_has_active_shared_claim(context.project.service_id(), issue_id)? {
		return Ok(false);
	}
	if context.dispatch_mode == IssueDispatchMode::Closeout {
		return closeout_lane_active_claim_blocks_dispatch(
			context.project,
			context.state_store,
			issue,
		);
	}

	Ok(true)
}

pub(crate) fn closeout_lane_active_claim_blocks_dispatch(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Result<bool> {
	if !state_store.issue_has_active_shared_claim(project.service_id(), &issue.id)? {
		return Ok(false);
	}

	let Some(lease) = state_store.lease_for_issue(&issue.id)? else {
		return Ok(true);
	};
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

	retained_closeout_lease_has_fresh_activity(&lease, issue, project, now_unix_epoch)
}

fn target_issue_run_context_with_dispatch_mode<'a, T>(
	context: &TargetIssueRunContext<'a, T>,
	dispatch_mode: IssueDispatchMode,
) -> TargetIssueRunContext<'a, T> {
	TargetIssueRunContext {
		tracker: context.tracker,
		project: context.project,
		workflow: context.workflow,
		state_store: context.state_store,
		issue_id: context.issue_id,
		preferred_issue_state: context.preferred_issue_state,
		preferred_initial_issue_state: context.preferred_initial_issue_state,
		dry_run: context.dry_run,
		lease_preacquired: context.lease_preacquired,
		preferred_issue_claim_fd: context.preferred_issue_claim_fd,
		preferred_dispatch_slot_fd: context.preferred_dispatch_slot_fd,
		preferred_dispatch_slot_index: context.preferred_dispatch_slot_index,
		dispatch_mode,
		preferred_run_identity: context.preferred_run_identity,
		preferred_retry_budget_base: context.preferred_retry_budget_base,
	}
}

fn resolve_target_issue_id<T>(tracker: &T, issue_reference: &str) -> Result<String>
where
	T: IssueTracker,
{
	if commit_message::looks_like_issue_identifier(issue_reference)
		&& let Some(issue) = tracker.get_issue_by_identifier(issue_reference)?
	{
		return Ok(issue.id);
	}

	Ok(issue_reference.to_owned())
}

fn adopt_preacquired_target_issue_lease<T>(
	context: &TargetIssueRunContext<'_, T>,
	issue_id: &str,
) -> Result<()>
where
	T: IssueTracker,
{
	let preferred_run_identity = context.preferred_run_identity.ok_or_else(|| {
		eyre::eyre!("daemon child lease handoff requires a planned run identifier")
	})?;
	let preferred_issue_state = context
		.preferred_issue_state
		.ok_or_else(|| eyre::eyre!("daemon child lease handoff requires a planned issue state"))?;
	let issue_claim_fd = context.preferred_issue_claim_fd.ok_or_else(|| {
		eyre::eyre!("daemon child lease handoff requires an inherited issue-claim fd")
	})?;
	let dispatch_slot_fd = context.preferred_dispatch_slot_fd.ok_or_else(|| {
		eyre::eyre!("daemon child lease handoff requires an inherited dispatch-slot fd")
	})?;
	let dispatch_slot_index = context.preferred_dispatch_slot_index.ok_or_else(|| {
		eyre::eyre!("daemon child lease handoff requires an inherited dispatch-slot index")
	})?;

	context.state_store.adopt_preacquired_lease(
		context.project.service_id(),
		issue_id,
		preferred_run_identity.run_id,
		preferred_issue_state,
		PreacquiredLeaseGuards { issue_claim_fd, dispatch_slot_fd, dispatch_slot_index },
	)?;

	Ok(())
}

pub(crate) fn prepare_issue_run<T>(
	context: PrepareIssueRunContext<'_, T>,
	issue: TrackerIssue,
) -> Result<Option<IssueRunPlan>>
where
	T: IssueTracker,
{
	let retained_closeout_worktree = retained_closeout_prepare_worktree(&context, &issue)?;
	let planned_worktree = retained_closeout_worktree
		.clone()
		.unwrap_or_else(|| context.worktree_manager.plan_for_issue(&issue.identifier));
	let Some((attempt_number, run_id)) =
		resolve_prepare_run_identity(context.state_store, &issue, context.preferred_run_identity)?
	else {
		return Ok(None);
	};
	let retry_budget_base = retry_budget_base_for_dispatch_mode(
		context.state_store,
		&issue.id,
		&planned_worktree.path,
		context.dispatch_mode,
		context.preferred_retry_budget_base,
	)?;
	let lease_issue_id = issue.id.clone();
	let issue_state = planned_issue_state_for_dispatch(
		context.workflow,
		&issue,
		context.dispatch_mode,
		context.preferred_issue_state,
	);

	validate_workflow_read_first_files(context.project, context.workflow)?;

	if !context.dry_run
		&& !context.lease_preacquired
		&& !context.state_store.try_acquire_lease(
			context.project.service_id(),
			&issue.id,
			&run_id,
			&issue_state,
		)? {
		return Ok(None);
	}

	match (|| -> Result<Option<IssueRunPlan>> {
		let worktree = if let Some(worktree) = retained_closeout_worktree.clone() {
			worktree
		} else {
			context.worktree_manager.ensure_worktree_with_hooks(
				&issue.identifier,
				context.dry_run,
				context.workflow.frontmatter().execution().workspace_hooks(),
			)?
		};

		if !context.dry_run {
			context.state_store.upsert_worktree(
				context.project.service_id(),
				&lease_issue_id,
				&worktree.branch_name,
				&worktree.path.display().to_string(),
			)?;
		}

		let Some(refreshed_issue) = refresh_issue(context.tracker, &lease_issue_id)? else {
			return Ok(None);
		};

		if !prepare_issue_run_dispatch_allowed(
			&context,
			&refreshed_issue,
			&lease_issue_id,
			&worktree.branch_name,
			&worktree.path,
		)? {
			return Ok(None);
		}
		if !context.dry_run {
			record_starting_attempt(context.state_store, &run_id, &issue.id, attempt_number)?;
			clear_terminal_guard_marker(&worktree.path)?;
		}

		let initial_issue_state = context
			.preferred_initial_issue_state
			.map_or_else(|| refreshed_issue.state.name.clone(), str::to_owned);
		let issue_run = IssueRunPlan {
			issue: refreshed_issue,
			issue_state: issue_state.clone(),
			initial_issue_state,
			worktree,
			#[cfg(test)]
			retry_project_slug: String::new(),
			dispatch_mode: context.dispatch_mode,
			attempt_number,
			run_id: run_id.clone(),
			retry_budget_base,
		};

		if !context.dry_run {
			write_prepare_lifecycle_events(
				context.tracker,
				context.project,
				context.workflow,
				context.state_store,
				&issue_run,
			)?;
		}

		Ok(Some(issue_run))
	})() {
		Ok(Some(issue_run)) => Ok(Some(issue_run)),
		Ok(None) => {
			clear_prepare_issue_run_lease(context.state_store, context.dry_run, &lease_issue_id)?;

			Ok(None)
		},
		Err(error) => {
			clear_prepare_issue_run_lease(context.state_store, context.dry_run, &lease_issue_id)?;

			Err(error)
		},
	}
}

fn retained_closeout_prepare_worktree<T>(
	context: &PrepareIssueRunContext<'_, T>,
	issue: &TrackerIssue,
) -> Result<Option<WorktreeSpec>>
where
	T: IssueTracker,
{
	if context.dispatch_mode != IssueDispatchMode::Closeout {
		return Ok(None);
	}

	let Some(worktree) = context.state_store.worktree_for_issue(&issue.id)? else {
		return Ok(None);
	};
	if worktree.project_id() != context.project.service_id()
		|| !worktree.worktree_path().try_exists()?
	{
		return Ok(None);
	}

	Ok(Some(WorktreeSpec {
		branch_name: worktree.branch_name().to_owned(),
		issue_identifier: issue.identifier.clone(),
		path: worktree.worktree_path().to_path_buf(),
		reused_existing: true,
	}))
}

fn prepare_issue_run_dispatch_allowed<T>(
	context: &PrepareIssueRunContext<'_, T>,
	refreshed_issue: &TrackerIssue,
	lease_issue_id: &str,
	worktree_branch_name: &str,
	worktree_path: &Path,
) -> Result<bool>
where
	T: IssueTracker,
{
	let dispatch_allowed = context.dispatch_mode.allows_issue(
		context.tracker,
		refreshed_issue,
		context.project,
		context.workflow,
		context.state_store,
		RetryIssueStateHint {
			preferred_issue_state: context.preferred_issue_state,
			preferred_initial_issue_state: context.preferred_initial_issue_state,
		},
	)?;

	if !dispatch_allowed {
		if !context.dry_run
			&& context.dispatch_mode == IssueDispatchMode::Closeout
			&& let Some(reason) = closeout_dispatch_block_reason(
				context.tracker,
				refreshed_issue,
				context.project,
				context.workflow,
				context.state_store,
			)? {
			eyre::bail!("retained closeout dispatch blocked: {reason}");
		}
		if !context.dry_run && is_terminal_issue(refreshed_issue, context.workflow) {
			cleanup_terminal_worktree(
				context.state_store,
				context.worktree_manager,
				context.workflow,
				lease_issue_id,
				&refreshed_issue.identifier,
				worktree_branch_name,
				worktree_path,
			)?;
		}
	}

	Ok(dispatch_allowed)
}

fn clear_prepare_issue_run_lease(
	state_store: &StateStore,
	dry_run: bool,
	issue_id: &str,
) -> Result<()> {
	if !dry_run {
		state_store.clear_lease(issue_id)?;
	}

	Ok(())
}

fn record_starting_attempt(
	state_store: &StateStore,
	run_id: &str,
	issue_id: &str,
	attempt_number: i64,
) -> Result<()> {
	state_store.record_run_attempt(run_id, issue_id, attempt_number, "starting")
}

fn resolve_prepare_run_identity(
	state_store: &StateStore,
	issue: &TrackerIssue,
	preferred_run_identity: Option<PreferredRunIdentity<'_>>,
) -> Result<Option<(i64, String)>> {
	let next_attempt_number = state_store.next_attempt_number(&issue.id)?;

	match preferred_run_identity {
		Some(preferred_run_identity) => {
			if next_attempt_number > preferred_run_identity.attempt_number {
				let Some(existing_attempt) =
					state_store.run_attempt(preferred_run_identity.run_id)?
				else {
					return Ok(None);
				};

				if existing_attempt.issue_id() != issue.id
					|| existing_attempt.attempt_number() != preferred_run_identity.attempt_number
				{
					return Ok(None);
				}
			}

			Ok(Some((
				preferred_run_identity.attempt_number,
				preferred_run_identity.run_id.to_owned(),
			)))
		},
		None =>
			Ok(Some((next_attempt_number, build_run_id(&issue.identifier, next_attempt_number)?))),
	}
}

fn complete_issue_run<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: IssueRunPlan,
	dry_run: bool,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	if dry_run {
		return Ok(Some(run_summary_from_issue_run(project.service_id(), &issue_run)));
	}

	let summary = execute_issue_run(tracker, project, workflow, state_store, issue_run)?;
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};
	let summary = if let Some(retained_summary) =
		drain_non_github_review_retained_tail_with_inspector(
			tracker,
			project,
			workflow,
			state_store,
			&summary,
			&review_state_inspector,
			|source_summary| {
				run_retained_closeout_for_handoff_summary(
					tracker,
					project,
					workflow,
					state_store,
					source_summary,
				)
			},
		)? {
		retained_summary
	} else {
		summary
	};

	reconcile_terminal_thread_archive_backlog_best_effort(project, workflow, state_store);

	Ok(Some(summary))
}

pub(crate) fn run_retained_closeout_for_handoff_summary<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	source_summary: &RunSummary,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	run_target_issue_once(TargetIssueRunContext {
		tracker,
		project,
		workflow,
		state_store,
		issue_id: source_summary.issue_id.as_str(),
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: false,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
}

pub(crate) fn drain_non_github_review_retained_tail_with_inspector<T, I, F>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	summary: &RunSummary,
	review_state_inspector: &I,
	mut run_closeout: F,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
	F: FnMut(&RunSummary) -> Result<Option<RunSummary>>,
{
	if project.codex().review_level().uses_github_review()
		|| summary.continuation_pending
		|| !matches!(
			summary.dispatch_mode,
			IssueDispatchMode::Normal
				| IssueDispatchMode::Program
				| IssueDispatchMode::ReviewRepair
		) {
		return Ok(None);
	}

	let completed_state = workflow.frontmatter().tracker().resolved_completed_state();

	for pass in 0..INTERNAL_RETAINED_DRAIN_MAX_PASSES {
		reconcile_post_review_orchestration_with_inspector(
			tracker,
			project,
			workflow,
			state_store,
			review_state_inspector,
		)?;

		let Some(lane) = build_post_review_lane_statuses(
			tracker,
			project,
			workflow,
			state_store,
			review_state_inspector,
		)?
		.into_iter()
		.find(|lane| lane.issue_id == summary.issue_id) else {
			return Ok(None);
		};

		if post_review_lane_is_closeout_candidate(&lane, completed_state) {
			if let Some(retained_summary) = run_closeout(summary)? {
				return Ok(Some(retained_summary));
			}

			return Ok(None);
		}
		if lane.reason != "non_github_review_waiting_for_merge"
			|| pass + 1 == INTERNAL_RETAINED_DRAIN_MAX_PASSES
		{
			return Ok(None);
		}
	}

	Ok(None)
}
