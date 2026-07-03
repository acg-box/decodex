pub(crate) mod inferred;
pub(crate) mod post_review;
pub(crate) mod program;

pub(crate) use inferred::run_target_issue_once_with_inferred_dispatch;
#[cfg(test)] pub(crate) use program::select_target_status_visible_program_candidate;

use crate::{
	commit_message,
	orchestrator::{
		run_cycle,
		run_cycle::{
			IssueDispatchMode, IssueTracker, OffsetDateTime, PreferredRunIdentity,
			PrepareIssueRunContext, Result, RetainedReviewRunIdentity, RetryIssueStateHint,
			RunSummary, ServiceConfig, StateStore, TargetIssueRunContext, TrackerIssue,
			WorktreeManager, eyre,
		},
	},
	state::PreacquiredLeaseGuards,
};

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
		run_cycle::recover_runtime_state_from_tracker_and_worktrees(
			context.tracker,
			context.project,
			context.workflow,
			context.state_store,
		)?;

		if !context.dry_run {
			run_cycle::reconcile_project_state(
				context.tracker,
				context.project,
				context.workflow,
				context.state_store,
				&worktree_manager,
			)?;
		}
	}

	let Some(issue) = run_cycle::refresh_issue(context.tracker, &issue_id)? else {
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

	if !run_cycle::issue_passes_current_dispatch_policy(
		context.tracker,
		&issue,
		context.project,
		context.workflow,
		context.state_store,
		context.dispatch_mode,
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
		run_cycle::ensure_project_has_no_merged_worktree_cleanup_debt(context.project)?;
	}

	let Some(issue_run) = run_cycle::prepare_issue_run(
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

	run_cycle::complete_issue_run(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		issue_run,
		context.dry_run,
	)
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

	run_cycle::retained_closeout_lease_has_fresh_activity(&lease, issue, project, now_unix_epoch)
}

pub(crate) fn target_issue_run_context_with_dispatch_mode<'a, T>(
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

pub(crate) fn resolve_target_issue_id<T>(tracker: &T, issue_reference: &str) -> Result<String>
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

	let Some(reason) = run_cycle::closeout_dispatch_block_reason(
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

	run_cycle::retained_closeout_preferred_run_identity(
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
