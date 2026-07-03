#[cfg(unix)] use std::os::fd::AsRawFd;
use std::{
	env,
	io::Write as _,
	path::Path,
	process::{Child, Command, Stdio},
};

use crate::{
	cli::AttemptRequest,
	orchestrator::{
		self, DaemonRunChild, IssueDispatchMode, IssueTracker, MaterializedDaemonSpawnState,
		PullRequestReviewStateInspector, RUN_OPERATION_AGENT_RUN, Result, RetryDispatchDecision,
		RetryQueue, RunSummary, ServiceConfig, SpawnRunOnceChildRequest, StateStore,
		WorkflowDocument, WorktreeManager, WorktreeSpec,
		daemon::{self, DaemonTickRuntimeContext},
	},
	prelude::eyre,
	state,
};

pub(super) fn spawn_next_daemon_child<T>(
	config_path: &Path,
	state_store: &StateStore,
	active_children: &mut Vec<DaemonRunChild>,
	retry_queue: &mut RetryQueue,
	context: &DaemonTickRuntimeContext<'_, T, impl PullRequestReviewStateInspector>,
) -> Result<bool>
where
	T: IssueTracker,
{
	let next_run = plan_next_daemon_run(
		retry_queue,
		context.tracker,
		context.project,
		context.workflow,
		state_store,
	)?;

	match next_run {
		Some((summary, from_retry_queue)) => {
			if summary.dispatch_mode != IssueDispatchMode::Closeout {
				orchestrator::ensure_project_has_no_merged_worktree_cleanup_debt(context.project)?;
			}

			orchestrator::validate_workflow_read_first_files(context.project, context.workflow)?;

			state_store.configure_dispatch_slot_root(
				context.project.service_id(),
				context.project.worktree_root(),
			)?;

			if !state_store.try_acquire_lease(
				context.project.service_id(),
				&summary.issue_id,
				&summary.run_id,
				&summary.issue_state,
			)? {
				return Ok(false);
			}

			let daemon_spawn_state = materialize_daemon_spawn_state(
				context.project,
				context.workflow,
				state_store,
				&summary,
			)
			.inspect_err(|_error| {
				let _ = state_store.clear_lease(&summary.issue_id);
			})?;

			state_store.record_run_attempt(
				&summary.run_id,
				&summary.issue_id,
				summary.attempt_number,
				"starting",
			)?;
			state_store.upsert_worktree(
				context.project.service_id(),
				&summary.issue_id,
				&daemon_spawn_state.worktree.branch_name,
				&daemon_spawn_state.worktree.path.display().to_string(),
			)?;

			let mut child = spawn_planned_daemon_child(
				config_path,
				state_store,
				context.workflow,
				&summary,
				daemon_spawn_state.retry_budget_base,
			)?;

			if let Err(error) = state::write_run_operation_marker_for_process(
				&daemon_spawn_state.worktree.path,
				&summary.run_id,
				summary.attempt_number,
				child.id(),
				RUN_OPERATION_AGENT_RUN,
			) {
				let _ = child.kill();
				let _ = child.wait();
				let _ = state_store.update_run_status(&summary.run_id, "failed");
				let _ = state_store.clear_lease(&summary.issue_id);

				return Err(error);
			}

			state_store.update_run_status(&summary.run_id, "running")?;

			tracing::info!(
				issue = summary.issue_identifier,
				worktree = %daemon_spawn_state.worktree.path.display(),
				retry = from_retry_queue,
				"Spawned control-plane child for current issue lane."
			);

			active_children.push(DaemonRunChild {
				child,
				issue_id: summary.issue_id,
				run_id: summary.run_id,
				attempt_number: summary.attempt_number,
				initial_issue_state: summary.initial_issue_state,
				#[cfg(test)]
				retry_project_slug: String::new(),
				dispatch_mode: summary.dispatch_mode,
				from_retry_queue,
				workflow: context.workflow.clone(),
			});

			Ok(true)
		},
		None => {
			if retry_queue.is_empty() {
				tracing::debug!("Daemon tick found no eligible issue.");
			} else {
				tracing::debug!("Daemon tick is holding a queued retry claim.");
			}

			Ok(false)
		},
	}
}

pub(super) fn spawn_planned_daemon_child(
	config_path: &Path,
	state_store: &StateStore,
	workflow: &WorkflowDocument,
	summary: &RunSummary,
	retry_budget_base: i64,
) -> Result<Child> {
	let issue_claim_handoff =
		Some(state_store.clone_issue_claim_for_child(&summary.issue_id).inspect_err(|_error| {
			let _ = state_store.update_run_status(&summary.run_id, "failed");
			let _ = state_store.clear_lease(&summary.issue_id);
		})?);
	let (dispatch_slot_handoff_file, dispatch_slot_index) =
		state_store.clone_dispatch_slot_for_child(&summary.issue_id)?;
	let dispatch_slot_handoff = Some(dispatch_slot_handoff_file);
	let dispatch_slot_index_handoff = Some(dispatch_slot_index);
	let mut child = spawn_run_once_child(SpawnRunOnceChildRequest {
		config_path,
		preferred_issue_id: summary.issue_id.as_str(),
		preferred_issue_state: summary.issue_state.as_str(),
		preferred_initial_issue_state: Some(summary.initial_issue_state.as_str()),
		dispatch_mode: summary.dispatch_mode,
		preferred_run_id: summary.run_id.as_str(),
		preferred_attempt_number: summary.attempt_number,
		preferred_retry_budget_base: retry_budget_base,
		workflow,
		issue_claim_handoff: issue_claim_handoff.as_ref(),
		dispatch_slot_handoff: dispatch_slot_handoff.as_ref(),
		dispatch_slot_index_handoff,
	})
	.inspect_err(|_error| {
		let _ = state_store.update_run_status(&summary.run_id, "failed");
		let _ = state_store.clear_lease(&summary.issue_id);
	})?;

	state_store.release_handed_off_guards(&summary.issue_id).inspect_err(|_error| {
		let _ = child.kill();
		let _ = child.wait();
		let _ = state_store.update_run_status(&summary.run_id, "failed");
		let _ = state_store.clear_lease(&summary.issue_id);
	})?;

	Ok(child)
}

pub(super) fn spawn_run_once_child(request: SpawnRunOnceChildRequest<'_>) -> Result<Child> {
	let executable = env::current_exe()?;
	let lease_preacquired =
		request.issue_claim_handoff.is_some() || request.dispatch_slot_handoff.is_some();
	let attempt_request = AttemptRequest {
		dry_run: false,
		issue_id: String::from(request.preferred_issue_id),
		issue_state: String::from(request.preferred_issue_state),
		initial_issue_state: request.preferred_initial_issue_state.map(String::from),
		lease_preacquired,
		#[cfg(unix)]
		issue_claim_fd: request.issue_claim_handoff.map(AsRawFd::as_raw_fd),
		#[cfg(not(unix))]
		issue_claim_fd: None,
		#[cfg(unix)]
		dispatch_slot_fd: request.dispatch_slot_handoff.map(AsRawFd::as_raw_fd),
		#[cfg(not(unix))]
		dispatch_slot_fd: None,
		dispatch_slot_index: request.dispatch_slot_index_handoff,
		dispatch_mode: request.dispatch_mode.into(),
		run_id: String::from(request.preferred_run_id),
		attempt_number: request.preferred_attempt_number,
		retry_budget_base: request.preferred_retry_budget_base,
		workflow_snapshot: request.workflow.to_markdown()?,
	};
	let payload = serde_json::to_vec(&attempt_request)?;
	let mut command = Command::new(executable);

	command
		.args(["_attempt", "--config"])
		.arg(request.config_path)
		.arg("-")
		.stdin(Stdio::piped())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit());

	let mut child = command.spawn()?;
	let Some(mut stdin) = child.stdin.take() else {
		let _ = child.kill();
		let _ = child.wait();

		eyre::bail!("Spawned `_attempt` child without a writable stdin handle.");
	};

	if let Err(error) = stdin.write_all(&payload) {
		let _ = child.kill();
		let _ = child.wait();

		eyre::bail!("Failed to write `_attempt` request payload: {error}");
	}

	Ok(child)
}

pub(crate) fn plan_next_daemon_run<T>(
	retry_queue: &mut RetryQueue,
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<Option<(RunSummary, bool)>>
where
	T: IssueTracker,
{
	match daemon::plan_due_retry_run(retry_queue, tracker, project, workflow, state_store)? {
		RetryDispatchDecision::Dispatch(summary) => Ok(Some((*summary, true))),
		RetryDispatchDecision::Blocked { excluded_issue_ids } => {
			let excluded_issue_ids =
				excluded_issue_ids.iter().map(String::as_str).collect::<Vec<_>>();
			let issue_run = orchestrator::plan_project_issue_run_with_exclusions(
				tracker,
				project,
				workflow,
				state_store,
				true,
				&excluded_issue_ids,
			)?;

			Ok(issue_run.map(|issue_run| {
				(orchestrator::run_summary_from_issue_run(project.service_id(), &issue_run), false)
			}))
		},
		RetryDispatchDecision::Continue => {
			let issue_run = orchestrator::plan_project_issue_run_with_exclusions(
				tracker,
				project,
				workflow,
				state_store,
				true,
				&[],
			)?;

			Ok(issue_run.map(|issue_run| {
				(orchestrator::run_summary_from_issue_run(project.service_id(), &issue_run), false)
			}))
		},
	}
}

pub(crate) fn materialize_daemon_spawn_state(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	summary: &RunSummary,
) -> Result<MaterializedDaemonSpawnState> {
	let worktree = materialize_run_summary_worktree(project, workflow, summary)?;
	let retry_budget_base = orchestrator::retry_budget_base_for_dispatch_mode(
		state_store,
		&summary.issue_id,
		&worktree.path,
		summary.dispatch_mode,
		None,
	)?;

	Ok(MaterializedDaemonSpawnState { worktree, retry_budget_base })
}

pub(crate) fn materialize_run_summary_worktree(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	summary: &RunSummary,
) -> Result<WorktreeSpec> {
	if summary.dispatch_mode == IssueDispatchMode::Closeout {
		if !summary.worktree_path.try_exists()? {
			eyre::bail!(
				"planned retained closeout worktree `{}` is missing for issue `{}`",
				summary.worktree_path.display(),
				summary.issue_identifier
			);
		}

		return Ok(WorktreeSpec {
			branch_name: summary.branch_name.clone(),
			issue_identifier: summary.issue_identifier.clone(),
			path: summary.worktree_path.clone(),
			reused_existing: true,
		});
	}

	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());
	let worktree = worktree_manager.ensure_worktree_with_hooks(
		&summary.issue_identifier,
		false,
		workflow.frontmatter().execution().workspace_hooks(),
	)?;

	if worktree.path != summary.worktree_path {
		eyre::bail!(
			"planned worktree path `{}` diverged from materialized path `{}` for issue `{}`",
			summary.worktree_path.display(),
			worktree.path.display(),
			summary.issue_identifier
		);
	}
	if worktree.branch_name != summary.branch_name {
		eyre::bail!(
			"planned branch `{}` diverged from materialized branch `{}` for issue `{}`",
			summary.branch_name,
			worktree.branch_name,
			summary.issue_identifier
		);
	}

	Ok(worktree)
}
