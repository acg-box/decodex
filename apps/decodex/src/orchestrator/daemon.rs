mod active_children;
mod retry_dispatch;
mod spawn;

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
pub(crate) use active_children::{
	clear_orphaned_daemon_child_state, resolve_child_exit_run_attempt,
};

use daemon_retry::{
	clear_retry_schedule_and_release, retry_entry_is_temporarily_blocked,
	schedule_retry_after_child_exit,
};

use crate::{
	cli::AttemptRequest,
	orchestrator::{
		self, AccountActivityMode, ActiveWorkflowOverride, AsRawFd, CachedWorkflowDocument, Child,
		ChildExitRetryContext, ChildRunRef, Command, CurrentChildRunContext, DaemonRunChild,
		DaemonTickContext, GhPullRequestReviewStateInspector, Instant, IssueDispatchMode,
		IssueTracker, LinearClient, MaterializedDaemonSpawnState, OffsetDateTime,
		OperatorConnectorBackoffStatus, OperatorStatusSnapshot, Path,
		PullRequestReviewStateInspector, RUN_OPERATION_AGENT_RUN, RecoverableWorktreeSkipCache,
		Result, RetryDispatchDecision, RetryKind, RetryQueue, RunAttempt, RunLeaseDisposition,
		RunLeaseReconciliation, RunSummary, ServiceConfig, SpawnRunOnceChildRequest, StateStore,
		Stdio, TargetIssueRunContext, WorkflowDocument, WorktreeManager, WorktreeSpec, Write,
		apply_run_lease_reconciliation, daemon_retry,
		ensure_project_has_no_merged_worktree_cleanup_debt, env, eyre,
		inspect_exited_daemon_child_reconciliation, is_issue_not_dispatchable_for_current_dispatch,
		is_terminal_issue, mark_run_attempt_if_active, plan_project_issue_run_with_exclusions,
		refresh_issue, retained_review_handoff_matches_run, retry_budget_base_for_dispatch_mode,
		run_lease_reconciliation_workflow, run_summary_from_issue_run, run_target_issue_once,
		stalled_idle_duration, stalled_run_has_retained_partial_progress,
		superseded_run_disposition, terminal_issue_keeps_retained_closeout,
		validate_workflow_read_first_files,
	},
};
#[cfg(not(test))] use retry_dispatch::plan_due_retry_run;

pub(crate) struct DaemonTickRuntimeContext<'a, T, I> {
	pub(crate) tracker: &'a T,
	pub(crate) project: &'a ServiceConfig,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) worktree_manager: &'a WorktreeManager,
	pub(crate) review_state_inspector: &'a I,
	pub(crate) recoverable_worktree_skip_cache: Option<&'a mut RecoverableWorktreeSkipCache>,
}

pub(crate) fn load_daemon_tick_context(
	config_path: &Path,
	workflow_cache: &mut Option<CachedWorkflowDocument>,
) -> Result<DaemonTickContext> {
	let config = ServiceConfig::from_path(config_path)?;
	let workflow = load_daemon_tick_workflow(&config, workflow_cache)?;
	let api_key = config.tracker().resolve_api_key()?;
	let tracker = LinearClient::new(api_key)?;
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	Ok(DaemonTickContext { config, workflow, tracker, worktree_manager })
}

pub(crate) fn load_daemon_tick_workflow(
	config: &ServiceConfig,
	workflow_cache: &mut Option<CachedWorkflowDocument>,
) -> Result<WorkflowDocument> {
	let workflow_path = config.workflow_path().to_path_buf();
	let cached_same_path = workflow_cache
		.as_ref()
		.filter(|cached| cached.path == workflow_path)
		.map(|cached| cached.document.clone());

	match WorkflowDocument::from_path(&workflow_path) {
		Ok(workflow) => {
			if cached_same_path.as_ref().is_some_and(|cached| cached != &workflow) {
				tracing::info!(
					workflow_path = %workflow_path.display(),
					"Reloaded project WORKFLOW.md for future control-plane decisions."
				);
			}

			*workflow_cache =
				Some(CachedWorkflowDocument { path: workflow_path, document: workflow.clone() });

			Ok(workflow)
		},
		Err(error) =>
			if let Some(cached_workflow) = cached_same_path {
				tracing::warn!(
					workflow_path = %workflow_path.display(),
					?error,
					"Failed to reload project WORKFLOW.md; keeping the last known good workflow active for control-plane decisions."
				);

				Ok(cached_workflow)
			} else {
				Err(error)
			},
	}
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

pub(crate) fn build_operator_state_snapshot_for_publish<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
	warnings: &[&str],
	connector_backoffs: &[OperatorConnectorBackoffStatus],
) -> Result<OperatorStatusSnapshot>
where
	T: IssueTracker,
{
	let mut snapshot = if warnings.is_empty() {
		orchestrator::build_control_plane_operator_status_snapshot(
			tracker,
			project,
			workflow,
			state_store,
			limit,
		)?
	} else {
		orchestrator::build_operator_status_snapshot_with_account_mode(
			project,
			state_store,
			limit,
			AccountActivityMode::Snapshot,
		)?
	};

	if !warnings.is_empty() {
		orchestrator::hydrate_history_lanes_from_local_ledger(project, state_store, &mut snapshot)?;
	}

	orchestrator::apply_terminal_history_ledger_outcomes(&mut snapshot);

	if orchestrator::warnings_include_tracker_backoff(warnings) {
		let review_state_inspector = GhPullRequestReviewStateInspector {
			github_token_env_var: Some(project.github().token_env_var().to_owned()),
			github_command_path: project.github().command_path().map(Path::to_path_buf),
		};

		snapshot.post_review_lanes = orchestrator::build_degraded_post_review_lane_statuses(
			project,
			state_store,
			&review_state_inspector,
		)?;
	}

	for warning in warnings {
		orchestrator::add_operator_snapshot_warning(&mut snapshot, warning);
	}

	snapshot.connector_backoffs.extend(connector_backoffs.iter().cloned());

	if !warnings.is_empty() {
		orchestrator::add_operator_snapshot_warning(
			&mut snapshot,
			"external_observer_status_skipped",
		);
	}

	orchestrator::refresh_operator_project_summary(&mut snapshot, None);

	Ok(snapshot)
}
