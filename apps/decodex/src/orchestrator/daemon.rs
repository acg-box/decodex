use std::path::Path;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		AccountActivityMode, CachedWorkflowDocument, DaemonRunChild, DaemonTickContext,
		GhPullRequestReviewStateInspector, IssueTracker, OperatorConnectorBackoffStatus,
		OperatorStatusSnapshot, PullRequestReviewStateInspector, RecoverableWorktreeSkipCache,
		Result, RetryQueue, StateStore, WorkflowDocument, WorktreeManager,
		add_operator_snapshot_warning, apply_terminal_history_ledger_outcomes,
		build_control_plane_operator_status_snapshot, build_degraded_post_review_lane_statuses,
		build_operator_status_snapshot_with_account_mode,
		hydrate_history_lanes_from_local_ledger, reconcile_post_review_orchestration_with_inspector,
		reconcile_project_state, reconcile_terminal_thread_archive_backlog_best_effort,
		recover_runtime_state_from_tracker_and_worktrees_with_skip_cache,
		refresh_operator_project_summary, warnings_include_tracker_backoff,
	},
	tracker::linear::LinearClient,
};

mod active_children;
mod retry_dispatch;
mod spawn;

#[cfg(not(test))] use active_children::inspect_or_clear_active_children;
pub(crate) use active_children::{
	clear_orphaned_daemon_child_state, resolve_child_exit_run_attempt,
};
#[cfg(test)]
pub(crate) use active_children::{
	inspect_current_daemon_child_reconciliation, inspect_current_daemon_child_reconciliation_at,
	inspect_or_clear_active_children,
};
#[cfg(not(test))] use retry_dispatch::plan_due_retry_run;
#[cfg(test)] pub(crate) use retry_dispatch::plan_due_retry_run;
use spawn::spawn_next_daemon_child;
#[cfg(test)]
pub(crate) use spawn::{
	materialize_daemon_spawn_state, materialize_run_summary_worktree, plan_next_daemon_run,
};

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
	inspect_or_clear_active_children(
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

	reconcile_post_review_orchestration_with_inspector(
		context.tracker,
		context.project,
		context.workflow,
		state_store,
		context.review_state_inspector,
	)?;
	reconcile_terminal_thread_archive_backlog_best_effort(
		context.project,
		context.workflow,
		state_store,
	);

	loop {
		if !spawn_next_daemon_child(
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
	let _ = recover_runtime_state_from_tracker_and_worktrees_with_skip_cache(
		tracker,
		project,
		workflow,
		state_store,
		recoverable_worktree_skip_cache,
	)?;

	reconcile_project_state(tracker, project, workflow, state_store, worktree_manager)
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
		build_control_plane_operator_status_snapshot(
			tracker,
			project,
			workflow,
			state_store,
			limit,
		)?
	} else {
		build_operator_status_snapshot_with_account_mode(
			project,
			state_store,
			limit,
			AccountActivityMode::Snapshot,
		)?
	};

	if !warnings.is_empty() {
		hydrate_history_lanes_from_local_ledger(project, state_store, &mut snapshot)?;
	}

	apply_terminal_history_ledger_outcomes(&mut snapshot);

	if warnings_include_tracker_backoff(warnings) {
		let review_state_inspector = GhPullRequestReviewStateInspector {
			github_token_env_var: Some(project.github().token_env_var().to_owned()),
			github_command_path: project.github().command_path().map(Path::to_path_buf),
		};

		snapshot.post_review_lanes = build_degraded_post_review_lane_statuses(
			project,
			state_store,
			&review_state_inspector,
		)?;
	}

	for warning in warnings {
		add_operator_snapshot_warning(&mut snapshot, warning);
	}

	snapshot.connector_backoffs.extend(connector_backoffs.iter().cloned());

	if !warnings.is_empty() {
		add_operator_snapshot_warning(&mut snapshot, "external_observer_status_skipped");
	}

	refresh_operator_project_summary(&mut snapshot, None);

	Ok(snapshot)
}
