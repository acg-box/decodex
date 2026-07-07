mod active_children;
mod context;
mod publish;
mod retry_dispatch;
mod spawn;

#[cfg(test)]
pub(crate) use self::context::load_daemon_tick_workflow;
pub(crate) use self::{
	active_children::{clear_orphaned_daemon_child_state, resolve_child_exit_run_attempt},
	context::load_daemon_tick_context,
	publish::build_operator_state_snapshot_for_publish,
};
#[cfg(test)]
pub(crate) use self::{
	active_children::{
		inspect_current_daemon_child_reconciliation,
		inspect_current_daemon_child_reconciliation_at, inspect_or_clear_active_children,
	},
	retry_dispatch::plan_due_retry_run,
	spawn::{
		materialize_daemon_spawn_state, materialize_run_summary_worktree, plan_next_daemon_run,
	},
};

use daemon_retry::{
	clear_retry_schedule_and_release, retry_entry_is_temporarily_blocked,
	schedule_retry_after_child_exit,
};

use crate::orchestrator::{
	self, ActiveWorkflowOverride, Child, ChildExitRetryContext, ChildRunRef,
	CurrentChildRunContext, DaemonRunChild, DaemonTickContext, GhPullRequestReviewStateInspector,
	Instant, IssueDispatchMode, IssueTracker, OffsetDateTime, Path,
	PullRequestReviewStateInspector, RecoverableWorktreeSkipCache, Result, RetryDispatchDecision,
	RetryKind, RetryQueue, RunAttempt, RunLeaseDisposition, RunLeaseReconciliation, ServiceConfig,
	StateStore, TargetIssueRunContext, WorkflowDocument, WorktreeManager,
	apply_run_lease_reconciliation, daemon_retry, inspect_exited_daemon_child_reconciliation,
	is_issue_not_dispatchable_for_current_dispatch, is_terminal_issue, mark_run_attempt_if_active,
	refresh_issue, retained_review_handoff_matches_run, run_lease_reconciliation_workflow,
	run_target_issue_once, run_target_status_visible_program_once, stalled_idle_duration,
	stalled_run_has_retained_partial_progress, superseded_run_disposition,
	terminal_issue_keeps_retained_closeout,
};
#[cfg(not(test))]
use retry_dispatch::plan_due_retry_run;

pub(crate) struct DaemonTickRuntimeContext<'a, T, I> {
	pub(crate) tracker: &'a T,
	pub(crate) project: &'a ServiceConfig,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) worktree_manager: &'a WorktreeManager,
	pub(crate) review_state_inspector: &'a I,
	pub(crate) recoverable_worktree_skip_cache: Option<&'a mut RecoverableWorktreeSkipCache>,
}

pub(crate) fn run_daemon_tick(
	config_path: &Path,
	state_store: &StateStore,
	active_children: &mut Vec<DaemonRunChild>,
	retry_queue: &mut RetryQueue,
	recoverable_worktree_skip_cache: &mut RecoverableWorktreeSkipCache,
	context: &DaemonTickContext,
) -> Result<()> {
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(context.config.github().token_env_var().to_owned()),
		github_command_path: context.config.github().command_path().map(Path::to_path_buf),
	};

	run_daemon_tick_with_review_state_inspector(
		config_path,
		state_store,
		active_children,
		retry_queue,
		DaemonTickRuntimeContext {
			tracker: &context.tracker,
			project: &context.config,
			workflow: &context.workflow,
			worktree_manager: &context.worktree_manager,
			review_state_inspector: &review_state_inspector,
			recoverable_worktree_skip_cache: Some(recoverable_worktree_skip_cache),
		},
	)
}

pub(crate) fn run_daemon_tick_with_review_state_inspector<T, I>(
	config_path: &Path,
	state_store: &StateStore,
	active_children: &mut Vec<DaemonRunChild>,
	retry_queue: &mut RetryQueue,
	mut context: DaemonTickRuntimeContext<'_, T, I>,
) -> Result<()>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	active_children::inspect_or_clear_active_children(
		active_children,
		retry_queue,
		context.tracker,
		context.project,
		context.workflow,
		state_store,
		context.worktree_manager,
	)?;

	if active_children.is_empty() {
		let recoverable_worktree_skip_cache =
			context.recoverable_worktree_skip_cache.as_deref_mut();

		recover_and_reconcile_idle_daemon_state(
			context.tracker,
			context.project,
			context.workflow,
			state_store,
			context.worktree_manager,
			recoverable_worktree_skip_cache,
		)?;
	}

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		context.tracker,
		context.project,
		context.workflow,
		state_store,
		context.review_state_inspector,
	)?;
	orchestrator::reconcile_terminal_thread_archive_backlog_best_effort(
		context.project,
		context.workflow,
		state_store,
	);

	loop {
		if !spawn::spawn_next_daemon_child(
			config_path,
			state_store,
			active_children,
			retry_queue,
			&context,
		)? {
			break;
		}
	}

	Ok(())
}

pub(crate) fn recover_and_reconcile_idle_daemon_state<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	recoverable_worktree_skip_cache: Option<&mut RecoverableWorktreeSkipCache>,
) -> Result<()>
where
	T: IssueTracker,
{
	let _ = orchestrator::recover_runtime_state_from_tracker_and_worktrees_with_skip_cache(
		tracker,
		project,
		workflow,
		state_store,
		recoverable_worktree_skip_cache,
	)?;

	orchestrator::reconcile_project_state(tracker, project, workflow, state_store, worktree_manager)
}
