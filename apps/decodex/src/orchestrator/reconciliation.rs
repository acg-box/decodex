#[cfg(test)]
fn inspect_active_run_reconciliation_at<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	active_workflow_override: Option<ActiveWorkflowOverride<'_>>,
	now_unix_epoch: i64,
) -> Result<Vec<ActiveRunReconciliation>>
where
	T: IssueTracker,
{
	let leases = state_store.list_leases(project.service_id())?;

	if leases.is_empty() {
		return Ok(Vec::new());
	}

	let issue_ids = leases.iter().map(|lease| lease.issue_id().to_owned()).collect::<Vec<_>>();
	let issues = tracker.refresh_issues(&issue_ids)?;
	let issues_by_id =
		issues.into_iter().map(|issue| (issue.id.clone(), issue)).collect::<HashMap<_, _>>();
	let mut actions = Vec::new();

	for lease in leases {
		let Some(issue) = issues_by_id.get(lease.issue_id()).cloned() else {
			continue;
		};
		let Some(run_attempt) = state_store.run_attempt(lease.run_id())? else {
			continue;
		};
		let worktree_mapping = state_store.worktree_for_issue(&issue.id)?;
		let action_workflow = active_reconciliation_workflow_for_lease(
			workflow,
			active_workflow_override,
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
		let disposition =
			if let Some(disposition) = superseded_run_disposition(state_store, &run_attempt)? {
				Some(disposition)
			} else if !retained_closeout && is_terminal_issue(&issue, action_workflow) {
			Some(ActiveRunDisposition::Terminal)
		} else if !retained_closeout
			&& is_issue_nonactive_for_run(&issue, action_workflow)
		{
			Some(ActiveRunDisposition::NonActive)
		} else if let Some(idle_for) = stalled_idle_duration(
			state_store,
			&run_attempt,
			worktree_mapping.as_ref(),
			now_unix_epoch,
		)? {
			if retained_review_handoff_matches_run(
				state_store,
				&run_attempt,
				worktree_mapping.as_ref(),
			)? {
				Some(ActiveRunDisposition::RetainedReviewComplete)
			} else if stalled_run_has_retained_partial_progress(worktree_mapping.as_ref()) {
				Some(ActiveRunDisposition::StalledRetainedPartialProgress { idle_for })
			} else {
				Some(ActiveRunDisposition::Stalled { idle_for })
			}
		} else {
			None
		};

		if let Some(disposition) = disposition {
			actions.push(ActiveRunReconciliation {
				issue: issue.clone(),
				run_attempt,
				worktree_mapping,
				disposition,
				workflow: action_workflow.clone(),
			});
		}
	}

	Ok(actions)
}

fn inspect_exited_daemon_child_reconciliation<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
) -> Result<Vec<ActiveRunReconciliation>>
where
	T: IssueTracker,
{
	inspect_exited_daemon_child_reconciliation_at(
		tracker,
		project,
		workflow,
		state_store,
		issue_id,
		run_id,
		OffsetDateTime::now_utc().unix_timestamp(),
	)
}

fn inspect_exited_daemon_child_reconciliation_at<T>(
	tracker: &T,
	_project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
	now_unix_epoch: i64,
) -> Result<Vec<ActiveRunReconciliation>>
where
	T: IssueTracker,
{
	let Some(issue) = refresh_issue(tracker, issue_id)? else {
		return Ok(Vec::new());
	};
	let Some(run_attempt) = state_store.run_attempt(run_id)? else {
		return Ok(Vec::new());
	};
	let worktree_mapping = state_store.worktree_for_issue(issue_id)?;

	if let Some(disposition) = superseded_run_disposition(state_store, &run_attempt)? {
		return Ok(vec![ActiveRunReconciliation {
			issue,
			run_attempt,
			worktree_mapping,
			disposition,
			workflow: workflow.clone(),
		}]);
	}

	if run_attempt.status() != "failed" || !is_issue_active_for_run(&issue, workflow) {
		return Ok(Vec::new());
	}

	let Some(idle_for) = stalled_protocol_idle_duration(
		state_store,
		&run_attempt,
		worktree_mapping.as_ref(),
		now_unix_epoch,
	)?
	else {
		return Ok(Vec::new());
	};
	let disposition = if stalled_run_has_retained_partial_progress(worktree_mapping.as_ref()) {
		ActiveRunDisposition::StalledRetainedPartialProgress { idle_for }
	} else {
		ActiveRunDisposition::Stalled { idle_for }
	};

	Ok(vec![ActiveRunReconciliation {
		issue,
		run_attempt,
		worktree_mapping,
		disposition,
		workflow: workflow.clone(),
	}])
}

fn active_reconciliation_workflow_for_lease<'a>(
	current_workflow: &'a WorkflowDocument,
	active_workflow_override: Option<ActiveWorkflowOverride<'a>>,
	issue: &TrackerIssue,
	run_attempt: &RunAttempt,
) -> &'a WorkflowDocument {
	match active_workflow_override {
		Some(override_context)
			if override_context.child.issue_id == issue.id
				&& override_context.child.run_id == run_attempt.run_id() =>
			override_context.workflow,
		_ => current_workflow,
	}
}

fn apply_active_run_reconciliation<T>(
	tracker: &T,
	project: &ServiceConfig,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	actions: Vec<ActiveRunReconciliation>,
) -> Result<()>
where
	T: IssueTracker,
{
	for action in actions {
		match &action.disposition {
			ActiveRunDisposition::RetainedReviewComplete => {
				reconcile_retained_review_complete_active_run(project, state_store, &action)?;
			},
			ActiveRunDisposition::Superseded {
				newer_run_id,
				newer_attempt_number,
			} => {
				reconcile_superseded_active_run(
					project,
					state_store,
					&action,
					newer_run_id,
					*newer_attempt_number,
				)?;
			},
			ActiveRunDisposition::Terminal => {
				tracing::info!(
					project_id = project.service_id(),
					issue_id = action.issue.id,
					issue = action.issue.identifier,
					run_id = action.run_attempt.run_id(),
					disposition = "terminal",
					"Reconciling terminal active run."
				);

				mark_run_attempt_if_active(state_store, action.run_attempt.run_id(), "terminated")?;

				tracker::clear_automation_lane_labels(tracker, &action.issue, project.service_id())?;

				state_store.clear_lease(&action.issue.id)?;

				if let Some(mapping) = &action.worktree_mapping {
					cleanup_worktree_mapping(
						state_store,
						worktree_manager,
						&action.workflow,
						&action.issue.identifier,
						mapping,
					)?;
				}
			},
			ActiveRunDisposition::NonActive => {
				reconcile_nonactive_active_run(project, state_store, worktree_manager, &action)?;
			},
			ActiveRunDisposition::Stalled { idle_for } => {
				reconcile_stalled_active_run(
					tracker,
					project,
					state_store,
					worktree_manager,
					&action,
					*idle_for,
				)?;
			},
			ActiveRunDisposition::StalledRetainedPartialProgress { idle_for } => {
				reconcile_stalled_retained_partial_progress_run(
					tracker,
					project,
					state_store,
					worktree_manager,
					&action,
					*idle_for,
				)?;
			},
			ActiveRunDisposition::StalledAlreadyNeedsAttention { idle_for } => {
				reconcile_stalled_attention_run(project, state_store, &action, *idle_for)?;
			},
		}
	}

	Ok(())
}

fn reconcile_superseded_active_run(
	project: &ServiceConfig,
	state_store: &StateStore,
	action: &ActiveRunReconciliation,
	newer_run_id: &str,
	newer_attempt_number: i64,
) -> Result<()> {
	tracing::info!(
		project_id = project.service_id(),
		issue_id = action.issue.id,
		issue = action.issue.identifier,
		run_id = action.run_attempt.run_id(),
		attempt = action.run_attempt.attempt_number(),
		superseded_by_run_id = newer_run_id,
		superseded_by_attempt = newer_attempt_number,
		disposition = "superseded",
		"Reconciling superseded active run without tracker writeback."
	);

	mark_run_attempt_if_active(state_store, action.run_attempt.run_id(), "interrupted")?;

	if let Some(lease) = state_store.lease_for_issue(&action.issue.id)?
		&& lease.run_id() == action.run_attempt.run_id()
	{
		state_store.clear_lease(&action.issue.id)?;
	}

	Ok(())
}

fn reconcile_retained_review_complete_active_run(
	project: &ServiceConfig,
	state_store: &StateStore,
	action: &ActiveRunReconciliation,
) -> Result<()> {
	tracing::info!(
		project_id = project.service_id(),
		issue_id = action.issue.id,
		issue = action.issue.identifier,
		run_id = action.run_attempt.run_id(),
		disposition = "retained_review_complete",
		"Reconciling completed retained review run."
	);

	mark_run_attempt_if_active(state_store, action.run_attempt.run_id(), "succeeded")?;

	state_store.clear_lease(&action.issue.id)?;

	Ok(())
}

fn reconcile_nonactive_active_run(
	project: &ServiceConfig,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	action: &ActiveRunReconciliation,
) -> Result<()> {
	tracing::info!(
		project_id = project.service_id(),
		issue_id = action.issue.id,
		issue = action.issue.identifier,
		run_id = action.run_attempt.run_id(),
		disposition = "non_active",
		"Reconciling non-active run."
	);

	mark_run_attempt_if_active(state_store, action.run_attempt.run_id(), "interrupted")?;

	let worktree_path = action.worktree_mapping.as_ref().map_or_else(
		|| worktree_manager.plan_for_issue(&action.issue.identifier).path,
		|mapping| mapping.worktree_path().to_path_buf(),
	);

	if worktree_path.exists() {
		write_retry_budget_marker(
			&worktree_path,
			action.run_attempt.run_id(),
			action.run_attempt.attempt_number(),
			retry_budget_base_for_issue_worktree(state_store, &action.issue.id, &worktree_path)?,
		)?;
	}

	state_store.clear_lease(&action.issue.id)?;

	Ok(())
}

fn reconcile_stalled_active_run<T>(
	tracker: &T,
	project: &ServiceConfig,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	action: &ActiveRunReconciliation,
	idle_for: Duration,
) -> Result<()>
where
	T: IssueTracker,
{
	tracing::warn!(
		project_id = project.service_id(),
		issue_id = action.issue.id,
		issue = action.issue.identifier,
		run_id = action.run_attempt.run_id(),
		disposition = "stalled",
		idle_for_s = idle_for.as_secs(),
		"Reconciling stalled run."
	);

	state_store.update_run_status(action.run_attempt.run_id(), "stalled")?;
	state_store.clear_lease(&action.issue.id)?;

	let issue_run = stalled_reconciliation_issue_run(state_store, worktree_manager, action)?;

	write_reconciliation_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_RECONCILIATION,
	);
	handle_failure(
		tracker,
		project,
		&action.workflow,
		state_store,
		&issue_run,
		&Report::new(StalledRunNeedsAttention {
			issue_identifier: action.issue.identifier.clone(),
			run_id: action.run_attempt.run_id().to_owned(),
			idle_for,
		}),
	)?;

	Ok(())
}

fn reconcile_stalled_retained_partial_progress_run<T>(
	tracker: &T,
	project: &ServiceConfig,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	action: &ActiveRunReconciliation,
	idle_for: Duration,
) -> Result<()>
where
	T: IssueTracker,
{
	tracing::warn!(
		project_id = project.service_id(),
		issue_id = action.issue.id,
		issue = action.issue.identifier,
		run_id = action.run_attempt.run_id(),
		disposition = "stalled_retained_partial_progress",
		idle_for_s = idle_for.as_secs(),
		"Reconciling stalled run with retained partial progress."
	);

	let issue_run = stalled_reconciliation_issue_run(state_store, worktree_manager, action)?;
	let recovered = match try_recover_stalled_retained_phase_goal(
		project,
		&action.workflow,
		state_store,
		&action.issue,
		&issue_run,
	) {
		Ok(recovered) => recovered,
		Err(error) if run_failure_requires_terminal_attention(&error) => {
			state_store.update_run_status(action.run_attempt.run_id(), "stalled")?;
			state_store.clear_lease(&action.issue.id)?;

			handle_failure(
				tracker,
				project,
				&action.workflow,
				state_store,
				&issue_run,
				&error,
			)?;

			return Ok(());
		},
		Err(error) => return Err(error),
	};

	if recovered {
		return Ok(());
	}

	state_store.update_run_status(action.run_attempt.run_id(), "stalled")?;
	state_store.clear_lease(&action.issue.id)?;

	let worktree_path = relative_worktree_path(project, &issue_run.worktree);

	write_reconciliation_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_RECONCILIATION,
	);
	handle_failure(
		tracker,
		project,
		&action.workflow,
		state_store,
		&issue_run,
		&Report::new(RetainedPartialProgress {
			issue_identifier: action.issue.identifier.clone(),
			run_id: action.run_attempt.run_id().to_owned(),
			worktree_path,
			source_error_class: Some(String::from("stalled_run_detected")),
		}),
	)?;

	Ok(())
}

fn try_recover_stalled_retained_phase_goal(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
	issue_run: &IssueRunPlan,
) -> Result<bool> {
	write_reconciliation_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_RECONCILIATION,
	);

	let recovery = recover_active_phase_goal_continuation(
		project,
		workflow,
		state_store,
		issue_run,
		"stalled_run_detected",
	)?;
	let Some(recovery) = recovery else {
		return Ok(false);
	};

	state_store.update_run_status(&issue_run.run_id, CONTINUATION_PENDING_RUN_STATUS)?;
	state_store.clear_lease(&issue.id)?;

	write_stalled_phase_goal_continuation_retry_marker(state_store, workflow, issue_run)?;

	tracing::warn!(
		project_id = project.service_id(),
		issue_id = issue.id,
		issue = issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		source_phase = recovery.source_phase.as_str(),
		next_phase = recovery.next_phase.as_str(),
		"Recovered stalled retained phase goal; scheduling continuation instead of manual attention."
	);

	Ok(true)
}

fn write_stalled_phase_goal_continuation_retry_marker(
	state_store: &StateStore,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
) -> Result<()> {
	let attempt = u32::try_from(issue_run.attempt_number).unwrap_or(u32::MAX).max(1);
	let delay = retry_delay(RetryKind::Continuation, attempt, workflow);
	let retry_ready_at_unix_epoch = OffsetDateTime::now_utc().unix_timestamp().saturating_add(
		i64::try_from((delay.as_millis().saturating_add(999)) / 1_000).unwrap_or(i64::MAX),
	);

	write_retry_schedule_for_run(
		state_store,
		&issue_run.issue.id,
		&issue_run.run_id,
		issue_run.attempt_number,
		RetryKind::Continuation,
		retry_ready_at_unix_epoch,
	)
}

fn stalled_reconciliation_issue_run(
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	action: &ActiveRunReconciliation,
) -> Result<IssueRunPlan> {
	let worktree = action.worktree_mapping.as_ref().map_or_else(
		|| worktree_manager.plan_for_issue(&action.issue.identifier),
		|mapping| WorktreeSpec {
			branch_name: mapping.branch_name().to_owned(),
			issue_identifier: action.issue.identifier.clone(),
			path: mapping.worktree_path().to_path_buf(),
			reused_existing: true,
		},
	);
	let retry_budget_base =
		retry_budget_base_for_issue_worktree(state_store, &action.issue.id, &worktree.path)?;

	Ok(IssueRunPlan {
		issue: action.issue.clone(),
		issue_state: planned_issue_state_for_dispatch(
			&action.workflow,
			&action.issue,
			IssueDispatchMode::Retry,
			None,
		),
		initial_issue_state: action.issue.state.name.clone(),
		worktree,
		#[cfg(test)]
		retry_project_slug: String::new(),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: action.run_attempt.attempt_number(),
		run_id: action.run_attempt.run_id().to_owned(),
		retry_budget_base,
	})
}

fn reconcile_stalled_attention_run(
	project: &ServiceConfig,
	state_store: &StateStore,
	action: &ActiveRunReconciliation,
	idle_for: Duration,
) -> Result<()> {
	tracing::warn!(
		project_id = project.service_id(),
		issue_id = action.issue.id,
		issue = action.issue.identifier,
		run_id = action.run_attempt.run_id(),
		disposition = "stalled_already_needs_attention",
		idle_for_s = idle_for.as_secs(),
		"Reconciling stalled run that is already blocked for operator attention."
	);

	state_store.update_run_status(action.run_attempt.run_id(), "stalled")?;

	state_store.clear_lease(&action.issue.id)
}

fn write_reconciliation_operation_marker_best_effort(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	current_operation: &str,
) {
	if let Err(error) = state::write_run_operation_marker_preserving_activity(
		worktree_path,
		run_id,
		attempt_number,
		current_operation,
	) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			current_operation,
			worktree_path = %worktree_path.display(),
			"Run operation marker write failed; continuing stalled-run reconciliation."
		);
	}
}

fn stalled_run_has_retained_partial_progress(
	worktree_mapping: Option<&WorktreeMapping>,
) -> bool {
	match worktree_mapping {
		Some(mapping) => worktree_has_tracked_changes(mapping.worktree_path()),
		None => false,
	}
}

fn retained_review_handoff_matches_run(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<bool> {
	let Some(worktree_mapping) = worktree_mapping else {
		return Ok(false);
	};
	let Some(marker) = state_store.review_handoff_marker(
		worktree_mapping.project_id(),
		run_attempt.issue_id(),
		worktree_mapping.branch_name(),
	)? else {
		return Ok(false);
	};

	Ok(marker.run_id() == run_attempt.run_id()
		&& marker.attempt_number() == run_attempt.attempt_number()
		&& marker.branch_name() == worktree_mapping.branch_name())
}

fn superseded_run_disposition(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
) -> Result<Option<ActiveRunDisposition>> {
	let Some(latest_attempt) = state_store.latest_run_attempt_for_issue(run_attempt.issue_id())?
	else {
		return Ok(None);
	};

	if latest_attempt.attempt_number() <= run_attempt.attempt_number() {
		return Ok(None);
	}

	Ok(Some(ActiveRunDisposition::Superseded {
		newer_run_id: latest_attempt.run_id().to_owned(),
		newer_attempt_number: latest_attempt.attempt_number(),
	}))
}

fn stalled_idle_duration(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
	now_unix_epoch: i64,
) -> Result<Option<Duration>> {
	if !matches!(run_attempt.status(), "starting" | "running") {
		return Ok(None);
	}
	if stalled_reconciliation_deferred_by_marker(run_attempt, worktree_mapping)? {
		return Ok(None);
	}

	let Some(last_activity) =
		last_observed_run_activity_unix_epoch(state_store, run_attempt, worktree_mapping)?
	else {
		return Ok(None);
	};
	let Some(idle_for) = observed_idle_duration(last_activity, now_unix_epoch) else {
		return Ok(None);
	};
	let idle_timeout = active_run_idle_timeout(run_attempt, worktree_mapping)?;

	if idle_for >= idle_timeout {
		return Ok(Some(idle_for));
	}

	Ok(None)
}

fn active_run_idle_timeout(
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<Duration> {
	let Some(worktree_mapping) = worktree_mapping else {
		return Ok(ACTIVE_RUN_IDLE_TIMEOUT);
	};
	let Some(marker) = state::read_run_activity_marker_snapshot(worktree_mapping.worktree_path())?
	else {
		return Ok(ACTIVE_RUN_IDLE_TIMEOUT);
	};

	if marker.run_id() != run_attempt.run_id()
		|| marker.attempt_number() != run_attempt.attempt_number()
	{
		return Ok(ACTIVE_RUN_IDLE_TIMEOUT);
	}

	Ok(agent::protocol_activity_idle_timeout(
		marker.protocol_activity(),
		ACTIVE_RUN_IDLE_TIMEOUT,
	))
}

fn last_observed_run_activity_unix_epoch(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<Option<i64>> {
	let state_store_activity = state_store.last_run_activity_unix_epoch(run_attempt.run_id())?;
	let worktree_activity = match worktree_mapping {
		Some(mapping) => state::read_run_activity_marker(
			mapping.worktree_path(),
			run_attempt.run_id(),
			run_attempt.attempt_number(),
		)?,
		None => None,
	};

	Ok(match (state_store_activity, worktree_activity) {
		(Some(left), Some(right)) => Some(left.max(right)),
		(Some(activity), None) | (None, Some(activity)) => Some(activity),
		(None, None) => None,
	})
}

fn stalled_protocol_idle_duration(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
	now_unix_epoch: i64,
) -> Result<Option<Duration>> {
	if stalled_reconciliation_deferred_by_marker(run_attempt, worktree_mapping)? {
		return Ok(None);
	}

	let Some(last_activity) =
		last_observed_protocol_activity_unix_epoch(state_store, run_attempt, worktree_mapping)?
	else {
		return Ok(None);
	};
	let Some(idle_for) = observed_idle_duration(last_activity, now_unix_epoch) else {
		return Ok(None);
	};

	if idle_for >= ACTIVE_RUN_IDLE_TIMEOUT {
		return Ok(Some(idle_for));
	}

	Ok(None)
}

fn last_observed_protocol_activity_unix_epoch(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<Option<i64>> {
	let state_store_activity =
		state_store.last_protocol_activity_unix_epoch(run_attempt.run_id())?;
	let worktree_activity = match worktree_mapping {
		Some(mapping) => state::read_run_protocol_activity_marker(
			mapping.worktree_path(),
			run_attempt.run_id(),
			run_attempt.attempt_number(),
		)?,
		None => None,
	};

	Ok(match (state_store_activity, worktree_activity) {
		(Some(left), Some(right)) => Some(left.max(right)),
		(Some(activity), None) | (None, Some(activity)) => Some(activity),
		(None, None) => None,
	})
}

fn observed_idle_duration(last_activity_unix_epoch: i64, now_unix_epoch: i64) -> Option<Duration> {
	now_unix_epoch
		.checked_sub(last_activity_unix_epoch)
		.and_then(|idle_seconds| u64::try_from(idle_seconds).ok())
		.map(Duration::from_secs)
}

fn stalled_reconciliation_deferred_by_marker(
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<bool> {
	let Some(marker) = current_run_activity_marker(run_attempt, worktree_mapping)? else {
		return Ok(false);
	};

	if marker.retry_kind().is_some() {
		return Ok(true);
	}

	Ok(marker.current_operation() == Some(RUN_OPERATION_REPO_GATE)
		&& marker_process_is_alive(&marker))
}

fn current_run_activity_marker(
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<Option<RunActivityMarker>> {
	let Some(worktree_mapping) = worktree_mapping else {
		return Ok(None);
	};
	let Some(marker) = state::read_run_activity_marker_snapshot(worktree_mapping.worktree_path())?
	else {
		return Ok(None);
	};

	if marker.run_id() == run_attempt.run_id()
		&& marker.attempt_number() == run_attempt.attempt_number()
	{
		return Ok(Some(marker));
	}

	Ok(None)
}
