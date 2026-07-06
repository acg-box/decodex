mod child;
mod materialize;
mod plan;

#[cfg(test)]
pub(crate) use self::{
	materialize::{materialize_daemon_spawn_state, materialize_run_summary_worktree},
	plan::plan_next_daemon_run,
};

use std::path::Path;

use crate::{
	orchestrator::{
		self, DaemonRunChild, IssueDispatchMode, IssueTracker, PullRequestReviewStateInspector,
		RUN_OPERATION_AGENT_RUN, Result, RetryQueue, StateStore, daemon::DaemonTickRuntimeContext,
	},
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
	let next_run = plan::plan_next_daemon_run(
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

			let daemon_spawn_state = materialize::materialize_daemon_spawn_state(
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

			if let Some(program_dispatch) = summary.program_dispatch.as_ref() {
				orchestrator::record_program_dispatch_selected_for_summary(
					state_store,
					&summary,
					program_dispatch,
				)?;
			}

			let mut child = child::spawn_planned_daemon_child(
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
