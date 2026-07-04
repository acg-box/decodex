mod cleanup;
mod inspection;

pub(crate) use self::cleanup::{clear_orphaned_daemon_child_state, resolve_child_exit_run_attempt};
#[cfg(test)]
pub(crate) use self::inspection::{
	inspect_current_daemon_child_reconciliation, inspect_current_daemon_child_reconciliation_at,
};

use crate::orchestrator::daemon::{
	self, ChildExitRetryContext, ChildRunRef, CurrentChildRunContext, DaemonRunChild, IssueTracker,
	Result, RetryQueue, ServiceConfig, StateStore, WorkflowDocument, WorktreeManager,
};

pub(crate) fn inspect_or_clear_active_children<T>(
	active_children: &mut Vec<DaemonRunChild>,
	retry_queue: &mut RetryQueue,
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
) -> Result<()>
where
	T: IssueTracker,
{
	let mut index = 0;

	while index < active_children.len() {
		let child_exit_status = active_children[index].child.try_wait()?;
		let child_exited = child_exit_status.is_some();

		if child_exited && child_exit_status.is_some_and(|status| !status.success()) {
			daemon::mark_run_attempt_if_active(
				state_store,
				&active_children[index].run_id,
				"failed",
			)?;
		}

		let child_ref = ChildRunRef {
			issue_id: &active_children[index].issue_id,
			run_id: &active_children[index].run_id,
			attempt_number: active_children[index].attempt_number,
		};
		let actions = if child_exited {
			daemon::inspect_exited_daemon_child_reconciliation(
				tracker,
				project,
				workflow,
				state_store,
				child_ref.issue_id,
				child_ref.run_id,
			)?
		} else {
			inspection::inspect_current_daemon_child_reconciliation(
				tracker,
				project,
				workflow,
				state_store,
				CurrentChildRunContext {
					child: child_ref,
					workflow: &active_children[index].workflow,
					dispatch_mode: active_children[index].dispatch_mode,
				},
			)?
		};

		if actions.is_empty() {
			if child_exited {
				if child_exit_status.is_some_and(|status| status.success()) {
					daemon::mark_run_attempt_if_active(
						state_store,
						&active_children[index].run_id,
						"succeeded",
					)?;
				}

				let daemon_child = active_children.swap_remove(index);
				let child_ref = ChildRunRef {
					issue_id: &daemon_child.issue_id,
					run_id: &daemon_child.run_id,
					attempt_number: daemon_child.attempt_number,
				};

				cleanup::clear_orphaned_daemon_child_state(state_store, child_ref, false)?;

				if let Some(exit_status) = child_exit_status {
					daemon::schedule_retry_after_child_exit(
						ChildExitRetryContext {
							retry_queue,
							tracker,
							project,
							workflow,
							state_store,
						},
						child_ref,
						#[cfg(test)]
						"",
						&daemon_child.initial_issue_state,
						daemon_child.dispatch_mode,
						exit_status,
					)?;
				}

				continue;
			}

			index += 1;

			continue;
		}

		let mut daemon_child = active_children.swap_remove(index);

		if daemon_child.from_retry_queue {
			retry_queue.release(&daemon_child.issue_id);
		}
		if !child_exited {
			cleanup::stop_daemon_child(&mut daemon_child.child)?;
		}

		daemon::apply_run_lease_reconciliation(
			tracker,
			project,
			state_store,
			worktree_manager,
			actions,
		)?;
	}

	Ok(())
}
