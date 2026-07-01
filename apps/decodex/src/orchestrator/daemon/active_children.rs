#[allow(clippy::wildcard_imports)] use super::*;

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
			mark_run_attempt_if_active(state_store, &active_children[index].run_id, "failed")?;
		}

		let child_ref = ChildRunRef {
			issue_id: &active_children[index].issue_id,
			run_id: &active_children[index].run_id,
			attempt_number: active_children[index].attempt_number,
		};
		let actions = if child_exited {
			inspect_exited_daemon_child_reconciliation(
				tracker,
				project,
				workflow,
				state_store,
				child_ref.issue_id,
				child_ref.run_id,
			)?
		} else {
			inspect_current_daemon_child_reconciliation(
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
					mark_run_attempt_if_active(
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

				clear_orphaned_daemon_child_state(state_store, child_ref, false)?;

				if let Some(exit_status) = child_exit_status {
					schedule_retry_after_child_exit(
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
			stop_daemon_child(&mut daemon_child.child)?;
		}

		apply_run_lease_reconciliation(tracker, project, state_store, worktree_manager, actions)?;
	}

	Ok(())
}

pub(crate) fn inspect_current_daemon_child_reconciliation<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	child_context: CurrentChildRunContext<'_>,
) -> Result<Vec<RunLeaseReconciliation>>
where
	T: IssueTracker,
{
	inspect_current_daemon_child_reconciliation_at(
		tracker,
		project,
		workflow,
		state_store,
		child_context,
		OffsetDateTime::now_utc().unix_timestamp(),
	)
}

pub(crate) fn inspect_current_daemon_child_reconciliation_at<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	child_context: CurrentChildRunContext<'_>,
	now_unix_epoch: i64,
) -> Result<Vec<RunLeaseReconciliation>>
where
	T: IssueTracker,
{
	let child = child_context.child;
	let Some(issue) = refresh_issue(tracker, child.issue_id)? else {
		return Ok(Vec::new());
	};
	let Some(run_attempt) = state_store.run_attempt(child.run_id)? else {
		return Ok(Vec::new());
	};
	let worktree_mapping = state_store.worktree_for_issue(&issue.id)?;

	if let Some(disposition) = superseded_run_disposition(state_store, &run_attempt)? {
		return Ok(vec![RunLeaseReconciliation {
			issue: issue.clone(),
			run_attempt,
			worktree_mapping,
			disposition,
			workflow: workflow.clone(),
		}]);
	}

	let action_workflow = run_lease_reconciliation_workflow(
		workflow,
		Some(ActiveWorkflowOverride { child, workflow: child_context.workflow }),
		&issue,
		&run_attempt,
	);
	let retained_closeout = terminal_issue_keeps_retained_closeout(
		tracker,
		&issue,
		project,
		action_workflow,
		state_store,
	)?;
	let completed_closeout_child =
		matches!(child_context.dispatch_mode, IssueDispatchMode::Closeout)
			&& is_terminal_issue(&issue, action_workflow);
	let disposition = if !retained_closeout
		&& !completed_closeout_child
		&& is_terminal_issue(&issue, action_workflow)
	{
		Some(RunLeaseDisposition::Terminal)
	} else if !retained_closeout
		&& !completed_closeout_child
		&& is_issue_not_dispatchable_for_current_dispatch(
			tracker,
			&issue,
			project,
			action_workflow,
			child_context.dispatch_mode,
		)? {
		Some(RunLeaseDisposition::NotDispatchable)
	} else if let Some(idle_for) =
		stalled_idle_duration(state_store, &run_attempt, worktree_mapping.as_ref(), now_unix_epoch)?
	{
		if retained_review_handoff_matches_run(
			state_store,
			&run_attempt,
			worktree_mapping.as_ref(),
		)? {
			Some(RunLeaseDisposition::RetainedReviewComplete)
		} else if stalled_run_has_retained_partial_progress(worktree_mapping.as_ref()) {
			Some(RunLeaseDisposition::StalledRetainedPartialProgress { idle_for })
		} else {
			Some(RunLeaseDisposition::Stalled { idle_for })
		}
	} else {
		None
	};

	Ok(disposition.map_or_else(Vec::new, |disposition| {
		vec![RunLeaseReconciliation {
			issue: issue.clone(),
			run_attempt,
			worktree_mapping,
			disposition,
			workflow: action_workflow.clone(),
		}]
	}))
}

pub(crate) fn clear_orphaned_daemon_child_state(
	state_store: &StateStore,
	child: ChildRunRef<'_>,
	mark_interrupted: bool,
) -> Result<()> {
	let resolved_run_attempt = resolve_child_exit_run_attempt(state_store, child)?;

	if resolved_run_attempt.is_none() {
		tracing::debug!(
			issue_id = child.issue_id,
			run_id = child.run_id,
			attempt = child.attempt_number,
			"Daemon child exited without a matching recorded run attempt; skipping orphan cleanup."
		);
	}
	if mark_interrupted && let Some(run_attempt) = resolved_run_attempt.as_ref() {
		mark_run_attempt_if_active(state_store, run_attempt.run_id(), "interrupted")?;
	}

	let existing_lease = state_store.lease_for_issue(child.issue_id)?;
	let issue_unowned_or_matches_run = existing_lease.as_ref().is_none_or(|lease| {
		resolved_run_attempt
			.as_ref()
			.is_some_and(|run_attempt| lease.run_id() == run_attempt.run_id())
			|| lease.run_id() == child.run_id
	});

	if existing_lease.is_some() && issue_unowned_or_matches_run {
		state_store.clear_lease(child.issue_id)?;
	}
	if resolved_run_attempt.is_some()
		&& issue_unowned_or_matches_run
		&& let Some(mapping) = state_store.worktree_for_issue(child.issue_id)?
		&& !mapping.worktree_path().try_exists()?
	{
		tracing::debug!(
			issue_id = child.issue_id,
			run_id = child.run_id,
			attempt = child.attempt_number,
			branch = mapping.branch_name(),
			worktree_path = %mapping.worktree_path().display(),
			"Cleared daemon child worktree mapping after the checkout was removed."
		);

		state_store.clear_worktree(child.issue_id)?;
	}

	Ok(())
}

pub(crate) fn resolve_child_exit_run_attempt(
	state_store: &StateStore,
	child: ChildRunRef<'_>,
) -> Result<Option<RunAttempt>> {
	state_store.run_attempt(child.run_id)
}

fn stop_daemon_child(child: &mut Child) -> Result<()> {
	if child.try_wait()?.is_some() {
		return Ok(());
	}

	let _ = child.kill();
	let _ = child.wait();

	Ok(())
}
