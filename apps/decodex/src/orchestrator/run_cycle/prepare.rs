use crate::orchestrator::run_cycle::{
	self, IssueDispatchMode, IssueRunPlan, IssueTracker, Path, PreferredRunIdentity,
	PrepareIssueRunContext, Result, RetryIssueStateHint, StateStore, TrackerIssue, WorktreeSpec,
	eyre,
};

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
	let retry_budget_base = run_cycle::retry_budget_base_for_dispatch_mode(
		context.state_store,
		&issue.id,
		&planned_worktree.path,
		context.dispatch_mode,
		context.preferred_retry_budget_base,
	)?;
	let lease_issue_id = issue.id.clone();
	let issue_state = run_cycle::planned_issue_state_for_dispatch(
		context.workflow,
		&issue,
		context.dispatch_mode,
		context.preferred_issue_state,
	);

	run_cycle::validate_workflow_read_first_files(context.project, context.workflow)?;

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

		let Some(refreshed_issue) = run_cycle::refresh_issue(context.tracker, &lease_issue_id)?
		else {
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

			run_cycle::clear_terminal_guard_marker(&worktree.path)?;
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
			run_cycle::write_prepare_lifecycle_events(
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
	let dispatch_allowed = run_cycle::issue_passes_current_dispatch_policy(
		context.tracker,
		refreshed_issue,
		context.project,
		context.workflow,
		context.state_store,
		context.dispatch_mode,
		RetryIssueStateHint {
			preferred_issue_state: context.preferred_issue_state,
			preferred_initial_issue_state: context.preferred_initial_issue_state,
		},
	)?;

	if !dispatch_allowed {
		if !context.dry_run
			&& context.dispatch_mode == IssueDispatchMode::Closeout
			&& let Some(reason) = run_cycle::closeout_dispatch_block_reason(
				context.tracker,
				refreshed_issue,
				context.project,
				context.workflow,
				context.state_store,
			)? {
			eyre::bail!("retained closeout dispatch blocked: {reason}");
		}
		if !context.dry_run && run_cycle::is_terminal_issue(refreshed_issue, context.workflow) {
			run_cycle::cleanup_terminal_worktree(
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
		None => Ok(Some((
			next_attempt_number,
			run_cycle::build_run_id(&issue.identifier, next_attempt_number)?,
		))),
	}
}
