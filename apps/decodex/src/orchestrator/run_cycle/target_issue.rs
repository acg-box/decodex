pub(crate) mod inferred;
pub(crate) mod post_review;
pub(crate) mod program;

mod closeout;
mod context;
mod identity;
mod lease;

pub(crate) use self::{
	inferred::run_target_issue_once_with_inferred_dispatch,
	program::run_target_status_visible_program_once,
};
#[cfg(test)] pub(crate) use program::select_target_status_visible_program_candidate;

use crate::orchestrator::{
	self, BaselineGuardDispatchOutcome,
	run_cycle::{
		self, IssueDispatchMode, IssueRunPlan, IssueTracker, PrepareIssueRunContext, Result,
		RetryIssueStateHint, RunSummary, ServiceConfig, StateStore, TargetIssueRunContext,
		TrackerIssue, WorktreeManager, eyre,
	},
};

pub(crate) fn run_target_issue_once<T>(
	context: TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	run_target_issue_once_after_prepare(context, |_| Ok(()))
}

pub(crate) fn run_target_issue_once_after_prepare<T, F>(
	context: TargetIssueRunContext<'_, T>,
	after_prepare: F,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
	F: FnOnce(&IssueRunPlan) -> Result<()>,
{
	let Some(issue_run) = plan_target_issue_run(&context)? else {
		return Ok(None);
	};

	after_prepare(&issue_run)?;

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
	closeout::target_issue_active_claim_blocks_dispatch(context, issue_id, issue)
}

pub(crate) fn closeout_lane_active_claim_blocks_dispatch(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Result<bool> {
	closeout::closeout_lane_active_claim_blocks_dispatch(project, state_store, issue)
}

pub(crate) fn target_issue_run_context_with_dispatch_mode<'a, T>(
	context: &TargetIssueRunContext<'a, T>,
	dispatch_mode: IssueDispatchMode,
) -> TargetIssueRunContext<'a, T> {
	context::target_issue_run_context_with_dispatch_mode(context, dispatch_mode)
}

pub(crate) fn resolve_target_issue_id<T>(tracker: &T, issue_reference: &str) -> Result<String>
where
	T: IssueTracker,
{
	identity::resolve_target_issue_id(tracker, issue_reference)
}

fn plan_target_issue_run<T>(context: &TargetIssueRunContext<'_, T>) -> Result<Option<IssueRunPlan>>
where
	T: IssueTracker,
{
	let _project_binding = crate::orchestrator::dispatch_policy::attest_project_binding(
		context.state_store,
		context.project,
	)?;
	let worktree_manager = WorktreeManager::new(
		context.project.service_id(),
		context.project.repo_root(),
		context.project.worktree_root(),
	);

	context.state_store.configure_dispatch_slot_root(
		context.project.service_id(),
		context.project.worktree_root(),
	)?;

	let issue_id = identity::resolve_target_issue_id(context.tracker, context.issue_id)?;

	if !context.dry_run {
		context.state_store.canonicalize_issue_identity(context.issue_id, &issue_id)?;
	}
	if context.lease_preacquired && !context.dry_run {
		lease::adopt_preacquired_target_issue_lease(context, &issue_id)?;
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
	let closeout_preferred_run_identity =
		closeout::target_closeout_preferred_run_identity(context, &issue)?;
	let preferred_run_identity = closeout::preferred_run_identity_with_closeout_fallback(
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
		ensure_target_closeout_dispatch_is_unblocked(context, &issue)?;

		return Ok(None);
	}

	let _binding_attestation = crate::orchestrator::dispatch_policy::attest_issue_project_binding(
		context.state_store,
		context.project,
		&issue,
	)?;

	let reuses_existing_closeout_claim =
		closeout::target_issue_reuses_existing_closeout_claim(context, &issue_id, &issue)?;

	if target_issue_active_claim_blocks_dispatch(context, &issue_id, &issue)? {
		return Ok(None);
	}
	if !context.dry_run && context.dispatch_mode != IssueDispatchMode::Closeout {
		run_cycle::ensure_project_has_no_merged_worktree_cleanup_debt(context.project)?;
	}
	if orchestrator::ensure_clean_baseline_before_dispatch(
		context.project,
		context.workflow,
		context.state_store,
		context.dispatch_mode,
		context.dry_run,
	)? == BaselineGuardDispatchOutcome::NormalizedMain
	{
		return Ok(None);
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

	Ok(Some(issue_run))
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
