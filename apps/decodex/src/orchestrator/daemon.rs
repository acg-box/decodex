use crate::cli::AttemptRequest;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RetryEntryRetentionDecision {
	Retain,
	Drop,
	Block,
}

enum ChildExitPhaseGoalRecovery {
	None,
	Continuation(PhaseGoalRecoveryContinuation),
	Terminalized,
}

struct ChildExitRetrySchedule<'a> {
	project_id: &'a str,
	issue_id: &'a str,
	run_id: &'a str,
	attempt_number: i64,
	continuation_initial_issue_state: Option<String>,
	dispatch_mode: IssueDispatchMode,
	kind: RetryKind,
	attempt: u32,
}

struct DaemonTickRuntimeContext<'a, T, I> {
	tracker: &'a T,
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	worktree_manager: &'a WorktreeManager,
	review_state_inspector: &'a I,
	recoverable_worktree_skip_cache: Option<&'a mut RecoverableWorktreeSkipCache>,
}

fn load_daemon_tick_context(
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

fn load_daemon_tick_workflow(
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
		Err(error) => {
			if let Some(cached_workflow) = cached_same_path {
				tracing::warn!(
					workflow_path = %workflow_path.display(),
					?error,
					"Failed to reload project WORKFLOW.md; keeping the last known good workflow active for control-plane decisions."
				);

				Ok(cached_workflow)
			} else {
				Err(error)
			}
		},
	}
}

fn run_daemon_tick(
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

fn run_daemon_tick_with_review_state_inspector<T, I>(
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

fn recover_and_reconcile_idle_daemon_state<T>(
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

fn build_operator_state_snapshot_for_publish<T>(
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

fn inspect_or_clear_active_children<T>(
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

fn inspect_current_daemon_child_reconciliation<T>(
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

fn inspect_current_daemon_child_reconciliation_at<T>(
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

fn clear_orphaned_daemon_child_state(
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

fn resolve_child_exit_run_attempt(
	state_store: &StateStore,
	child: ChildRunRef<'_>,
) -> Result<Option<RunAttempt>> {
	state_store.run_attempt(child.run_id)
}

fn spawn_next_daemon_child<T>(
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
				ensure_project_has_no_merged_worktree_cleanup_debt(context.project)?;
			}

			validate_workflow_read_first_files(context.project, context.workflow)?;

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

fn spawn_planned_daemon_child(
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

fn plan_next_daemon_run<T>(
	retry_queue: &mut RetryQueue,
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<Option<(RunSummary, bool)>>
where
	T: IssueTracker,
{
	match plan_due_retry_run(retry_queue, tracker, project, workflow, state_store)? {
		RetryDispatchDecision::Dispatch(summary) => Ok(Some((*summary, true))),
		RetryDispatchDecision::Blocked { excluded_issue_ids } => {
			let excluded_issue_ids =
				excluded_issue_ids.iter().map(String::as_str).collect::<Vec<_>>();
			let issue_run = plan_project_issue_run_with_exclusions(
				tracker,
				project,
				workflow,
				state_store,
				true,
				&excluded_issue_ids,
			)?;

			Ok(issue_run.map(|issue_run| {
				(run_summary_from_issue_run(project.service_id(), &issue_run), false)
			}))
		},
		RetryDispatchDecision::Continue => {
			let issue_run = plan_project_issue_run_with_exclusions(
				tracker,
				project,
				workflow,
				state_store,
				true,
				&[],
			)?;

			Ok(issue_run.map(|issue_run| {
				(run_summary_from_issue_run(project.service_id(), &issue_run), false)
			}))
		},
	}
}

fn materialize_daemon_spawn_state(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	summary: &RunSummary,
) -> Result<MaterializedDaemonSpawnState> {
	let worktree = materialize_run_summary_worktree(project, workflow, summary)?;
	let retry_budget_base = retry_budget_base_for_dispatch_mode(
		state_store,
		&summary.issue_id,
		&worktree.path,
		summary.dispatch_mode,
		None,
	)?;

	Ok(MaterializedDaemonSpawnState { worktree, retry_budget_base })
}

fn materialize_run_summary_worktree(
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

fn spawn_run_once_child(request: SpawnRunOnceChildRequest<'_>) -> Result<Child> {
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

fn plan_due_retry_run<T>(
	retry_queue: &mut RetryQueue,
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<RetryDispatchDecision>
where
	T: IssueTracker,
{
	let now = Instant::now();
	let Some(first_entry) = retry_queue.next_entry().cloned() else {
		return Ok(RetryDispatchDecision::Continue);
	};

	if now < first_entry.ready_at {
		tracing::debug!(
			issue_id = first_entry.issue_id,
			retry_kind = ?first_entry.kind,
			retry_attempt = first_entry.attempt,
			"Retry queue is holding the project claim until the next retry is due."
		);

		return Ok(RetryDispatchDecision::Blocked {
			excluded_issue_ids: queued_retry_issue_ids(retry_queue),
		});
	}

	let mut blocked_issue_id = None;

	for entry in retry_queue.ordered_entries() {
		if now < entry.ready_at {
			break;
		}

		let preferred_issue_state = (entry.kind == RetryKind::Continuation
			&& !matches!(
				entry.dispatch_mode,
				IssueDispatchMode::ReviewRepair | IssueDispatchMode::Closeout
			))
		.then_some(workflow.frontmatter().tracker().in_progress_state());
		let Some(summary) = run_target_issue_once(TargetIssueRunContext {
			tracker,
			project,
			workflow,
			state_store,
			issue_id: &entry.issue_id,
			preferred_issue_state,
			preferred_initial_issue_state: entry.continuation_initial_issue_state.as_deref(),
			dry_run: true,
			lease_preacquired: false,
			preferred_issue_claim_fd: None,
			preferred_dispatch_slot_fd: None,
			preferred_dispatch_slot_index: None,
			dispatch_mode: entry.dispatch_mode,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		})?
		else {
			if retry_entry_is_temporarily_blocked(tracker, project, workflow, state_store, &entry)?
			{
				blocked_issue_id.get_or_insert_with(|| entry.issue_id.clone());

				continue;
			}

			clear_retry_schedule_and_release(retry_queue, state_store, &entry.issue_id)?;

			continue;
		};

		return Ok(RetryDispatchDecision::Dispatch(Box::new(summary)));
	}

	Ok(blocked_issue_id.map_or(RetryDispatchDecision::Continue, |_issue_id| {
		RetryDispatchDecision::Blocked { excluded_issue_ids: queued_retry_issue_ids(retry_queue) }
	}))
}

fn queued_retry_issue_ids(retry_queue: &RetryQueue) -> Vec<String> {
	retry_queue.ordered_entries().into_iter().map(|entry| entry.issue_id).collect()
}

fn evaluate_post_review_retention_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dispatch_mode: IssueDispatchMode,
) -> Result<RetryEntryRetentionDecision>
where
	T: IssueTracker + ?Sized,
{
	match dispatch_mode {
		IssueDispatchMode::ReviewRepair => {
			Ok(if issue_passes_review_repair_dispatch_policy(tracker, issue, project, workflow)? {
				RetryEntryRetentionDecision::Retain
			} else {
				RetryEntryRetentionDecision::Drop
			})
		},
		IssueDispatchMode::Closeout => Ok(match evaluate_closeout_dispatch_policy_with_inspector(
			tracker,
			issue,
			project,
			workflow,
			state_store,
			&GhPullRequestReviewStateInspector {
				github_token_env_var: Some(project.github().token_env_var().to_owned()),
				github_command_path: project.github().command_path().map(Path::to_path_buf),
			},
		)? {
			CloseoutDispatchEligibility::Eligible => RetryEntryRetentionDecision::Retain,
			CloseoutDispatchEligibility::Ineligible => RetryEntryRetentionDecision::Drop,
			CloseoutDispatchEligibility::Blocked(_) => RetryEntryRetentionDecision::Block,
		}),
		_ => Ok(RetryEntryRetentionDecision::Drop),
	}
}

fn evaluate_retry_entry_retention_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	entry: &RetryEntry,
) -> Result<RetryEntryRetentionDecision>
where
	T: IssueTracker + ?Sized,
{
	if issue_has_blocking_lane_decision_evidence(project, state_store, &issue.id)? {
		return Ok(RetryEntryRetentionDecision::Drop);
	}

	if matches!(entry.dispatch_mode, IssueDispatchMode::ReviewRepair | IssueDispatchMode::Closeout)
	{
		if entry.dispatch_mode == IssueDispatchMode::ReviewRepair
			&& issue_retry_budget_exhausted(workflow, state_store, &issue.id)?
		{
			return Ok(RetryEntryRetentionDecision::Drop);
		}

		return evaluate_post_review_retention_policy(
			tracker,
			issue,
			project,
			workflow,
			state_store,
			entry.dispatch_mode,
		);
	}

	let preferred_issue_state = (entry.kind == RetryKind::Continuation)
		.then_some(workflow.frontmatter().tracker().in_progress_state());

	if issue_passes_retry_retention_policy(
		tracker,
		issue,
		project,
		workflow,
		state_store,
		RetryIssueStateHint {
			preferred_issue_state,
			preferred_initial_issue_state: entry.continuation_initial_issue_state.as_deref(),
		},
	)? {
		Ok(RetryEntryRetentionDecision::Retain)
	} else {
		Ok(RetryEntryRetentionDecision::Drop)
	}
}

fn retry_entry_is_temporarily_blocked<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	entry: &RetryEntry,
) -> Result<bool>
where
	T: IssueTracker,
{
	let Some(issue) = refresh_issue(tracker, &entry.issue_id)? else {
		return Ok(false);
	};

	match evaluate_retry_entry_retention_policy(
		tracker,
		&issue,
		project,
		workflow,
		state_store,
		entry,
	)? {
		RetryEntryRetentionDecision::Drop => return Ok(false),
		RetryEntryRetentionDecision::Block => return Ok(true),
		RetryEntryRetentionDecision::Retain => {},
	}

	if state_store.issue_has_active_shared_claim(project.service_id(), &entry.issue_id)? {
		return Ok(true);
	}

	Ok(false)
}

fn schedule_retry_after_child_exit<T>(
	mut context: ChildExitRetryContext<'_, T>,
	child: ChildRunRef<'_>,
	#[cfg(test)] _retry_project_slug: &str,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
	exit_status: ExitStatus,
) -> Result<()>
where
	T: IssueTracker,
{
	let Some(run_attempt) = resolve_child_exit_run_attempt(context.state_store, child)? else {
		tracing::debug!(
			issue_id = child.issue_id,
			run_id = child.run_id,
			attempt = child.attempt_number,
			"Daemon child exited without a matching recorded run attempt; skipping retry scheduling."
		);

		return Ok(());
	};

	if !exit_status.success() {
		mark_run_attempt_if_active(context.state_store, run_attempt.run_id(), "failed")?;
	}

	let Some(run_attempt) = context.state_store.run_attempt(run_attempt.run_id())? else {
		return Ok(());
	};

	if superseded_run_disposition(context.state_store, &run_attempt)?.is_some() {
		clear_retry_schedule_and_release(context.retry_queue, context.state_store, child.issue_id)?;

		return Ok(());
	}

	let issue_id = run_attempt.issue_id();
	let Some(issue) = refresh_issue(context.tracker, issue_id)? else {
		clear_retry_schedule_and_release(context.retry_queue, context.state_store, issue_id)?;

		return Ok(());
	};
	let continuation_pending =
		exit_status.success() && run_attempt.status() == CONTINUATION_PENDING_RUN_STATUS;

	if !exit_status.success() && run_attempt.status() != "failed" {
		clear_retry_schedule_and_release(context.retry_queue, context.state_store, issue_id)?;

		return Ok(());
	}

	let retention_decision = child_exit_retry_retention_decision(
		&context,
		&issue,
		initial_issue_state,
		dispatch_mode,
		continuation_pending,
	)?;

	if retention_decision == RetryEntryRetentionDecision::Drop {
		clear_retry_schedule_and_release(context.retry_queue, context.state_store, issue_id)?;

		return Ok(());
	}

	let recovered_phase_goal_continuation = match recover_child_exit_phase_goal(
		&mut context,
		&issue,
		child,
		issue_id,
		initial_issue_state,
		dispatch_mode,
		exit_status.success(),
	)? {
		ChildExitPhaseGoalRecovery::None => None,
		ChildExitPhaseGoalRecovery::Continuation(recovery) => Some(recovery),
		ChildExitPhaseGoalRecovery::Terminalized => return Ok(()),
	};
	let (kind, attempt, continuation_initial_issue_state) = if continuation_pending {
		(
			RetryKind::Continuation,
			u32::try_from(run_attempt.attempt_number()).unwrap_or(u32::MAX).max(1),
			Some(initial_issue_state.to_owned()),
		)
	} else if recovered_phase_goal_continuation.is_some() {
		context
			.state_store
			.update_run_status(run_attempt.run_id(), CONTINUATION_PENDING_RUN_STATUS)?;

		(
			RetryKind::Continuation,
			u32::try_from(run_attempt.attempt_number()).unwrap_or(u32::MAX).max(1),
			Some(initial_issue_state.to_owned()),
		)
	} else if exit_status.success() {
		clear_retry_schedule_and_release(context.retry_queue, context.state_store, issue_id)?;

		return Ok(());
	} else {
		let retry_budget_attempts = child_exit_retry_budget_attempt_count(&context, &issue, child)?;
		let retry_budget_limit = child_exit_retry_budget_limit(&context, &issue, child)?;

		if retry_budget_attempts >= retry_budget_limit {
			return terminalize_exhausted_child_exit_retry(
				context,
				issue,
				child,
				initial_issue_state,
				dispatch_mode,
				retry_budget_attempts,
			);
		}

		(RetryKind::Failure, retry_budget_attempts, None)
	};
	let lane_snapshot = LaneDecisionSnapshot::child_exit_retry(
		issue.identifier.clone(),
		run_attempt.run_id().to_owned(),
		run_attempt.attempt_number(),
		dispatch_mode,
		kind == RetryKind::Continuation,
		Some(kind),
		0,
		false,
		false,
	);
	let lane_decision = decide_lane_next_action(&lane_snapshot);

	context.state_store.append_private_execution_event(
		context.project.service_id(),
		issue_id,
		run_attempt.run_id(),
		run_attempt.attempt_number(),
		"lane_decision",
		lane_snapshot.to_json(lane_decision.next_action, lane_decision.reason),
	)?;

	if lane_decision_blocks_automatic_execution(lane_decision.next_action) {
		clear_retry_schedule_and_release(context.retry_queue, context.state_store, issue_id)?;

		return Ok(());
	}

	queue_child_exit_retry(
		context.retry_queue,
		context.state_store,
		context.workflow,
		ChildExitRetrySchedule {
			project_id: context.project.service_id(),
			issue_id,
			run_id: run_attempt.run_id(),
			attempt_number: run_attempt.attempt_number(),
			continuation_initial_issue_state,
			dispatch_mode,
			kind,
			attempt,
		},
	)
}

fn queue_child_exit_retry(
	retry_queue: &mut RetryQueue,
	state_store: &StateStore,
	workflow: &WorkflowDocument,
	schedule: ChildExitRetrySchedule<'_>,
) -> Result<()> {
	let attempt = schedule.attempt.max(1);
	let delay = retry_delay(schedule.kind, attempt, workflow);

	tracing::info!(
		issue_id = schedule.issue_id,
		retry_kind = ?schedule.kind,
		retry_attempt = attempt,
		retry_delay_ms = delay.as_millis(),
		"Queued retry after control-plane child exit."
	);

	let retry_ready_at_unix_epoch = OffsetDateTime::now_utc().unix_timestamp().saturating_add(
		i64::try_from((delay.as_millis().saturating_add(999)) / 1_000).unwrap_or(i64::MAX),
	);

	write_retry_schedule_for_run(
		state_store,
		schedule.issue_id,
		schedule.run_id,
		schedule.attempt_number,
		schedule.kind,
		retry_ready_at_unix_epoch,
	)?;

	if schedule.kind == RetryKind::Continuation {
		state_store.append_private_execution_event(
			schedule.project_id,
			schedule.issue_id,
			schedule.run_id,
			schedule.attempt_number,
			"continuation_lineage",
			json!({
				"schema": "decodex.continuation_lineage/1",
				"continuation_of_run_id": schedule.run_id,
				"source_attempt_number": schedule.attempt_number,
				"phase_cursor": "issue_private_evidence",
				"retry_budget_consumed": false,
				"retry_schedule_attempt": attempt,
				"continuation_initial_issue_state": schedule.continuation_initial_issue_state.as_deref(),
				"dispatch_mode": schedule.dispatch_mode.as_str(),
				"next_retry_kind": schedule.kind.as_str(),
			}),
		)?;
	}

	retry_queue.upsert(RetryEntry {
		issue_id: schedule.issue_id.to_owned(),
		#[cfg(test)]
		retry_project_slug: String::new(),
		continuation_initial_issue_state: schedule.continuation_initial_issue_state,
		dispatch_mode: schedule.dispatch_mode,
		kind: schedule.kind,
		attempt,
		ready_at: Instant::now() + delay,
	});

	Ok(())
}

fn recover_child_exit_phase_goal<T>(
	context: &mut ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
	issue_id: &str,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
	exit_success: bool,
) -> Result<ChildExitPhaseGoalRecovery>
where
	T: IssueTracker,
{
	if exit_success {
		return Ok(ChildExitPhaseGoalRecovery::None);
	}

	let recovery = maybe_recover_child_exit_phase_goal_continuation(
		context,
		issue,
		child,
		initial_issue_state,
		dispatch_mode,
	)?;

	if matches!(recovery, ChildExitPhaseGoalRecovery::Terminalized) {
		clear_retry_schedule_and_release(context.retry_queue, context.state_store, issue_id)?;
	}

	Ok(recovery)
}

fn maybe_recover_child_exit_phase_goal_continuation<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
) -> Result<ChildExitPhaseGoalRecovery>
where
	T: IssueTracker,
{
	let worktree = child_exit_worktree_spec(context, issue)?;
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: initial_issue_state.to_owned(),
		worktree,
		#[cfg(test)]
		retry_project_slug: String::new(),
		dispatch_mode,
		attempt_number: child.attempt_number,
		run_id: child.run_id.to_owned(),
		retry_budget_base: 0,
	};
	let recovery = match recover_phase_goal_continuation(
		context.project,
		context.workflow,
		context.state_store,
		&issue_run,
		"child_exit_failed",
		Some("child_exit_failed"),
	) {
		Ok(recovery) => recovery,
		Err(error) if run_failure_requires_terminal_attention(&error) => {
			handle_failure(
				context.tracker,
				context.project,
				context.workflow,
				context.state_store,
				&issue_run,
				&error,
			)?;

			return Ok(ChildExitPhaseGoalRecovery::Terminalized);
		},
		Err(error) => return Err(error),
	};

	if let Some(recovery) = &recovery {
		tracing::warn!(
			project_id = context.project.service_id(),
			issue_id = issue.id,
			issue = issue.identifier,
			run_id = child.run_id,
			attempt = child.attempt_number,
			source_phase = recovery.source_phase.as_str(),
			next_phase = recovery.next_phase.as_str(),
			"Recovered phase goal after child exit failure; scheduling continuation."
		);
	}

	Ok(recovery.map_or(ChildExitPhaseGoalRecovery::None, |recovery| {
		ChildExitPhaseGoalRecovery::Continuation(recovery)
	}))
}

fn child_exit_retry_retention_decision<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
	continuation_pending: bool,
) -> Result<RetryEntryRetentionDecision>
where
	T: IssueTracker,
{
	if issue_has_blocking_lane_decision_evidence(context.project, context.state_store, &issue.id)? {
		return Ok(RetryEntryRetentionDecision::Drop);
	}

	if matches!(dispatch_mode, IssueDispatchMode::ReviewRepair | IssueDispatchMode::Closeout) {
		return evaluate_post_review_retention_policy(
			context.tracker,
			issue,
			context.project,
			context.workflow,
			context.state_store,
			dispatch_mode,
		);
	}

	let preferred_issue_state = continuation_pending
		.then_some(context.workflow.frontmatter().tracker().in_progress_state());

	if issue_passes_retry_retention_policy(
		context.tracker,
		issue,
		context.project,
		context.workflow,
		context.state_store,
		RetryIssueStateHint {
			preferred_issue_state,
			preferred_initial_issue_state: continuation_pending.then_some(initial_issue_state),
		},
	)? {
		Ok(RetryEntryRetentionDecision::Retain)
	} else {
		Ok(RetryEntryRetentionDecision::Drop)
	}
}

fn child_exit_retry_budget_attempt_count<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
) -> Result<u32>
where
	T: IssueTracker,
{
	let state_attempts = context.state_store.retry_budget_attempt_count(&issue.id)?.max(1);
	let worktree = child_exit_worktree_spec(context, issue)?;
	let Some(marker) = state::read_run_activity_marker_snapshot(&worktree.path)? else {
		return Ok(u32::try_from(state_attempts).unwrap_or(u32::MAX).max(1));
	};
	let marker_attempts = state::read_run_retry_budget_attempt_count(&worktree.path)?.unwrap_or(0);
	let marker_is_current_child =
		marker.run_id() == child.run_id && marker.attempt_number() == child.attempt_number;
	let marker_attempt_is_local = context.state_store.run_attempt(marker.run_id())?.is_some();
	let retry_budget_attempts =
		if marker_attempts > 0 && !marker_is_current_child && !marker_attempt_is_local {
			marker_attempts.saturating_add(state_attempts)
		} else {
			marker_attempts.max(state_attempts)
		};

	Ok(u32::try_from(retry_budget_attempts).unwrap_or(u32::MAX).max(1))
}

fn child_exit_retry_budget_limit<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
) -> Result<u32>
where
	T: IssueTracker,
{
	let max_attempts = context.workflow.frontmatter().execution().max_attempts();
	let worktree = child_exit_worktree_spec(context, issue)?;
	let Some(marker) = state::read_run_activity_marker_snapshot(&worktree.path)? else {
		return Ok(max_attempts);
	};

	if marker.run_id() == child.run_id
		&& marker.attempt_number() == child.attempt_number
		&& marker.retry_kind() == Some(ARCHITECTURE_RECOVERY_RETRY_KIND)
	{
		return Ok(
			max_attempts.saturating_add(u32::try_from(ARCHITECTURE_RECOVERY_BUDGET).unwrap_or(0))
		);
	}

	Ok(max_attempts)
}

fn terminalize_exhausted_child_exit_retry<T>(
	context: ChildExitRetryContext<'_, T>,
	issue: TrackerIssue,
	child: ChildRunRef<'_>,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
	retry_budget_attempts: u32,
) -> Result<()>
where
	T: IssueTracker,
{
	apply_child_exit_terminal_failure_writeback(
		&context,
		&issue,
		child,
		initial_issue_state,
		dispatch_mode,
		i64::from(retry_budget_attempts),
	)?;
	clear_retry_schedule_and_release(context.retry_queue, context.state_store, child.issue_id)?;

	Ok(())
}

fn apply_child_exit_terminal_failure_writeback<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
	retry_budget_attempts: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	let worktree = child_exit_worktree_spec(context, issue)?;
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: initial_issue_state.to_owned(),
		worktree,
		#[cfg(test)]
		retry_project_slug: String::new(),
		dispatch_mode,
		attempt_number: child.attempt_number,
		run_id: child.run_id.to_owned(),
		retry_budget_base: 0,
	};
	let worktree_path = relative_worktree_path(context.project, &issue_run.worktree);
	let error = if worktree_has_tracked_changes(&issue_run.worktree.path) {
		Report::new(RetainedPartialProgress {
			issue_identifier: issue.identifier.clone(),
			run_id: child.run_id.to_owned(),
			worktree_path: worktree_path.clone(),
			source_error_class: None,
		})
	} else {
		Report::msg(format!(
			"Daemon child `{}` for issue `{}` exited unsuccessfully after exhausting retry budget.",
			child.run_id, issue.identifier
		))
	};
	let privacy_classifier = configured_public_projection_privacy_classifier(context.project)?;
	let outcome = apply_terminal_failure_writeback(
		context.tracker,
		TerminalFailureWritebackRuntime {
			service_id: context.project.service_id(),
			state_store: Some(context.state_store),
			privacy_classifier: &privacy_classifier,
		},
		context.workflow,
		&issue_run,
		&worktree_path,
		false,
		&error,
	)?;

	if outcome.retry_guarded_by_state {
		write_terminal_guard_marker(
			&issue_run.worktree.path,
			&issue_run.run_id,
			issue_run.attempt_number,
		)?;

		context.state_store.update_run_status(&issue_run.run_id, TERMINAL_GUARDED_RUN_STATUS)?;
	}

	write_retry_budget_marker(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = issue.id,
		issue = issue.identifier,
		run_id = child.run_id,
		attempt = child.attempt_number,
		retry_budget_attempt = retry_budget_attempts,
		branch = issue_run.worktree.branch_name,
		worktree_path = %worktree_path,
		error_class = outcome.error_class,
		"Daemon child failed and now requires operator attention."
	);

	Ok(())
}

fn child_exit_worktree_spec<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
) -> Result<WorktreeSpec>
where
	T: IssueTracker,
{
	if let Some(mapping) = context.state_store.worktree_for_issue(&issue.id)? {
		return Ok(WorktreeSpec {
			branch_name: mapping.branch_name().to_owned(),
			issue_identifier: issue.identifier.clone(),
			path: mapping.worktree_path().to_path_buf(),
			reused_existing: true,
		});
	}

	let worktree_manager = WorktreeManager::new(
		context.project.service_id(),
		context.project.repo_root(),
		context.project.worktree_root(),
	);

	Ok(worktree_manager.plan_for_issue(&issue.identifier))
}

fn write_retry_schedule_for_run(
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	kind: RetryKind,
	retry_ready_at_unix_epoch: i64,
) -> Result<()> {
	let default_kind = match kind {
		RetryKind::Continuation => "continuation",
		RetryKind::Failure => "failure",
	};
	let retry_kind_label =
		preserved_retry_schedule_kind(state_store, issue_id, run_id, attempt_number, default_kind)?;

	if let Some(worktree) = state_store.worktree_for_issue(issue_id)? {
		state::write_run_retry_schedule(
			worktree.worktree_path(),
			run_id,
			attempt_number,
			&retry_kind_label,
			retry_ready_at_unix_epoch,
		)?;
	}

	Ok(())
}

fn preserved_retry_schedule_kind(
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_kind: &str,
) -> Result<String> {
	let Some(worktree) = state_store.worktree_for_issue(issue_id)? else {
		return Ok(default_kind.to_owned());
	};
	let Some(marker) = state::read_run_activity_marker_snapshot(worktree.worktree_path())? else {
		return Ok(default_kind.to_owned());
	};

	if marker.run_id() == run_id
		&& marker.attempt_number() == attempt_number
		&& let Some(retry_kind) = marker.retry_kind()
	{
		return Ok(retry_kind.to_owned());
	}

	Ok(default_kind.to_owned())
}

fn clear_retry_schedule_and_release(
	retry_queue: &mut RetryQueue,
	state_store: &StateStore,
	issue_id: &str,
) -> Result<()> {
	clear_worktree_retry_schedule(state_store, issue_id)?;

	retry_queue.release(issue_id);

	Ok(())
}

fn retry_delay(kind: RetryKind, attempt: u32, workflow: &WorkflowDocument) -> Duration {
	match kind {
		RetryKind::Continuation => Duration::from_millis(CONTINUATION_RETRY_DELAY_MS),
		RetryKind::Failure => {
			let exponent = attempt.saturating_sub(1).min(31);
			let multiplier = 1_u128 << exponent;
			let requested = u128::from(FAILURE_RETRY_BASE_DELAY_MS).saturating_mul(multiplier);
			let capped = requested
				.min(u128::from(workflow.frontmatter().execution().max_retry_backoff_ms()));

			Duration::from_millis(capped as u64)
		},
	}
}

fn stop_daemon_child(child: &mut Child) -> Result<()> {
	if child.try_wait()?.is_some() {
		return Ok(());
	}

	let _ = child.kill();
	let _ = child.wait();

	Ok(())
}
