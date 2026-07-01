#[allow(clippy::wildcard_imports)] use super::*;

use state::PreacquiredLeaseGuards;

use crate::commit_message;

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
