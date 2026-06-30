use agent::{
	AppServerThreadArchiveOutcome, AppServerThreadArchiveRequest, CodexAccountAuthFailure,
	CodexAccountPool, CodexAccountProvider,
};
use git_credentials::GitSigningConfig;



struct AgentGitCredentialEnvironment {
	process_env: AppServerProcessEnv,
}
impl AgentGitCredentialEnvironment {
	fn process_env(&self) -> &AppServerProcessEnv {
		&self.process_env
	}
}


struct CompletedAppServerRun<'a, T>
where
	T: IssueTracker,
{
	tracker: &'a T,
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	issue_run: &'a IssueRunPlan,
	tracker_tool_bridge: &'a TrackerToolBridge<'a>,
	process_env: &'a AppServerProcessEnv,
	transport: &'a str,
	run_result: &'a AppServerRunResult,
}

struct IssueAppServerRun<'a, T>
where
	T: IssueTracker,
{
	tracker: &'a T,
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	issue_run: &'a IssueRunPlan,
	tracker_tool_bridge: &'a TrackerToolBridge<'a>,
	review_context: &'a ReviewHandoffContext,
	process_env: &'a AppServerProcessEnv,
	transport: &'a str,
	continuation_guard: &'a dyn TurnContinuationGuard,
	decodex_tool_bridge: &'a DecodexToolBridge<'a>,
	phase_goal_controller: &'a dyn PhaseGoalController,
	codex_account_provider: Option<&'a dyn CodexAccountProvider>,
}

#[derive(Clone)]
struct ThreadArchiveCandidate {
	issue_id: String,
	issue_identifier: String,
	run_id: String,
	attempt_number: i64,
	thread_id: String,
	sequence_number: i64,
}

struct ThreadArchiveCandidateSource<'a> {
	run_id: &'a str,
	issue_id: &'a str,
	issue_identifier: &'a str,
	attempt_number: i64,
	thread_id: &'a str,
	sequence_number: Option<i64>,
}


enum IssueAppServerRunOutcome {
	Completed(AppServerRunResult),
	Finalized(RunSummary),
}

fn execute_issue_run<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: IssueRunPlan,
) -> Result<RunSummary>
where
	T: IssueTracker,
{
	tracing::info!(
		project_id = project.service_id(),
		issue_id = issue_run.issue.id,
		issue = issue_run.issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		branch = issue_run.worktree.branch_name,
		worktree_path = %relative_worktree_path(project, &issue_run.worktree),
		"Starting issue run."
	);

	state_store.upsert_worktree(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.worktree.branch_name,
		&issue_run.worktree.path.display().to_string(),
	)?;

	let result =
		ensure_automation_activity_label(tracker, &issue_run.issue, project.service_id(), true)
			.and_then(|_| {
				execute_issue_run_inner(tracker, project, workflow, state_store, &issue_run)
			});

	state_store.clear_lease(&issue_run.issue.id)?;

	match result {
		Ok(summary) => {
			persist_issue_run_outcome(state_store, &issue_run.run_id, &summary)?;

			if !summary.continuation_pending {
				state_store.clear_loop_guardrail_checkpoints_for_issue(
					project.service_id(),
					&issue_run.issue.id,
				)?;

				reconcile_terminal_thread_archive_backlog_best_effort(
					project,
					workflow,
					state_store,
				);
			}

			tracing::info!(
				project_id = project.service_id(),
				issue_id = issue_run.issue.id,
				issue = issue_run.issue.identifier,
				run_id = issue_run.run_id,
				attempt = issue_run.attempt_number,
				branch = issue_run.worktree.branch_name,
				worktree_path = %relative_worktree_path(project, &issue_run.worktree),
				"Completed issue run."
			);

			Ok(summary)
		},
		Err(error) => {
			state_store.update_run_status(&issue_run.run_id, "failed")?;

			handle_failure(tracker, project, workflow, state_store, &issue_run, &error)?;

			Err(error)
		},
	}
}

fn persist_issue_run_outcome(
	state_store: &StateStore,
	run_id: &str,
	summary: &RunSummary,
) -> Result<()> {
	state_store.update_run_status(
		run_id,
		if summary.continuation_pending { CONTINUATION_PENDING_RUN_STATUS } else { "succeeded" },
	)
}

fn resolve_resume_thread_id(
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<Option<String>> {
	if let Some(run_attempt) = state_store.run_attempt(&issue_run.run_id)?
		&& run_attempt.attempt_number() == issue_run.attempt_number
		&& let Some(thread_id) = run_attempt.thread_id()
	{
		return Ok(Some(thread_id.to_owned()));
	}

	let marker = state::read_run_activity_marker_snapshot(&issue_run.worktree.path)?;

	Ok(marker
		.filter(|marker| {
			marker.run_id() == issue_run.run_id
				&& marker.attempt_number() == issue_run.attempt_number
		})
		.and_then(|marker| marker.thread_id().map(str::to_owned)))
}

fn prepare_agent_git_credentials(
	project: &ServiceConfig,
	run_id: &str,
	worktree_path: &Path,
) -> Result<AgentGitCredentialEnvironment> {
	let github_token = project.github().resolve_token().map_err(|error| {
		Report::new(AgentGitCredentialsUnavailable {
			run_id: run_id.to_owned(),
			token_env_var: project.github().token_env_var().to_owned(),
		})
		.wrap_err(error)
	})?;
	let signing_config = GitSigningConfig::from_local_git_config(worktree_path)?;

	Ok(AgentGitCredentialEnvironment {
		process_env: AppServerProcessEnv::with_github_credentials_and_signing_config(
			project.github().token_env_var().to_owned(),
			github_token,
			signing_config,
		),
	})
}

fn configured_public_projection_privacy_classifier(
	project: &ServiceConfig,
) -> Result<tracker::privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier> {
	tracker::privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier::from_config(
		project.privacy_classifier(),
	)
}

fn build_closeout_review_state_inspector(
	project: &ServiceConfig,
) -> GhPullRequestReviewStateInspector {
	GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	}
}

fn execute_issue_run_inner<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<RunSummary>
where
	T: IssueTracker,
{
	let transport = workflow.frontmatter().agent().transport().to_owned();
	let privacy_classifier = configured_public_projection_privacy_classifier(project)?;
	let review_context = build_review_run_context(project, state_store, issue_run)?;
	let tracker_tool_bridge =
		TrackerToolBridge::with_run_context_state_store_and_privacy_classifier(
			tracker,
			&issue_run.issue,
			workflow,
			review_context.clone(),
			state_store,
			&privacy_classifier,
		);

	if let Some(summary) = maybe_execute_deterministic_closeout(
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		&tracker_tool_bridge,
		&review_context,
	)? {
		return Ok(summary);
	}

	write_git_credentials_operation_marker(issue_run);

	let agent_git_credentials =
		prepare_agent_git_credentials(project, &issue_run.run_id, &issue_run.worktree.path)?;
	let codex_account_pool =
		project.codex().accounts().map(CodexAccountPool::from_config).transpose()?;
	let closeout_review_state_inspector = build_closeout_review_state_inspector(project);
	let continuation_guard = build_issue_turn_continuation_guard(
		tracker,
		&tracker_tool_bridge,
		workflow,
		project,
		issue_run,
		Some(&closeout_review_state_inspector),
	);
	let decodex_tool_bridge = DecodexToolBridge::new(
		&tracker_tool_bridge,
		build_decodex_run_context(workflow, issue_run),
	);
	let phase_goal_controller =
		build_phase_goal_controller(project, workflow, state_store, issue_run);
	let run_result = match execute_issue_app_server_run(IssueAppServerRun {
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		tracker_tool_bridge: &tracker_tool_bridge,
		review_context: &review_context,
		process_env: agent_git_credentials.process_env(),
		transport: &transport,
		continuation_guard: &continuation_guard,
		decodex_tool_bridge: &decodex_tool_bridge,
		phase_goal_controller: &phase_goal_controller,
		codex_account_provider: codex_account_pool
			.as_ref()
			.map(|pool| pool as &dyn CodexAccountProvider),
	})? {
		IssueAppServerRunOutcome::Completed(run_result) => run_result,
		IssueAppServerRunOutcome::Finalized(summary) => return Ok(summary),
	};

	if run_result.continuation_pending {
		return Ok(continuation_boundary_summary(project, workflow, issue_run, &run_result));
	}

	finalize_completed_app_server_run(CompletedAppServerRun {
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		tracker_tool_bridge: &tracker_tool_bridge,
		process_env: agent_git_credentials.process_env(),
		transport: &transport,
		run_result: &run_result,
	})
}

fn execute_issue_app_server_run<T>(
	input: IssueAppServerRun<'_, T>,
) -> Result<IssueAppServerRunOutcome>
where
	T: IssueTracker,
{
	let run_result = match agent::execute_app_server_run(
		&AppServerRunRequest {
			project_id: input.project.service_id().to_owned(),
			run_id: input.issue_run.run_id.clone(),
			issue_id: input.issue_run.issue.id.clone(),
			attempt_number: input.issue_run.attempt_number,
			listen: input.transport.to_owned(),
			cwd: input.issue_run.worktree.path.display().to_string(),
			developer_instructions: build_run_developer_instructions(
				input.tracker,
				input.project,
				input.workflow,
				input.state_store,
				input.issue_run,
				input.review_context,
			)?,
			user_input: build_run_user_input(
				input.tracker,
				input.project,
				input.workflow,
				input.state_store,
				input.issue_run,
				input.review_context,
			),
			max_turns: input.workflow.frontmatter().execution().max_turns(),
			timeout: RUN_LEASE_IDLE_TIMEOUT,
			process_env: input.process_env.clone(),
			continuation_user_input: Some(build_issue_run_continuation_user_input(
				input.project,
				input.workflow,
				input.issue_run,
				input.review_context,
			)),
			activity_marker_path: Some(input.issue_run.worktree.path.clone()),
			resume_thread_id: resolve_resume_thread_id(input.state_store, input.issue_run)?,
			ephemeral_thread: false,
			command_exec_health_check: None,
			dynamic_tool_handler: Some(input.decodex_tool_bridge),
			continuation_guard: Some(input.continuation_guard),
			phase_goal_controller: Some(input.phase_goal_controller),
			codex_account_provider: input.codex_account_provider,
		},
		input.state_store,
	) {
		Ok(run_result) => run_result,
		Err(error) => {
			if let Some(summary) = maybe_finalize_after_terminalized_app_server_failure(
				input.tracker,
				input.project,
				input.workflow,
				input.state_store,
				input.issue_run,
				input.tracker_tool_bridge,
				&error,
			)? {
				return Ok(IssueAppServerRunOutcome::Finalized(summary));
			}

			if !input.tracker_tool_bridge.has_tracker_exit_signal()
				&& let Some(summary) = maybe_continue_after_phase_goal_recovery(
					input.project,
					input.workflow,
					input.state_store,
					input.issue_run,
					&error,
				)? {
				return Ok(IssueAppServerRunOutcome::Finalized(summary));
			}

			return Err(preserve_and_promote_app_server_run_failure(
				input.project,
				input.state_store,
				input.issue_run,
				input.workflow,
				input.tracker_tool_bridge.completion_disposition(),
				error,
			));
		},
	};

	Ok(IssueAppServerRunOutcome::Completed(run_result))
}

fn build_issue_run_continuation_user_input(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	review_context: &ReviewHandoffContext,
) -> String {
	build_continuation_user_input(
		&issue_run.issue,
		workflow,
		issue_run.dispatch_mode,
		review_context.recorded_pr_url.as_deref(),
		workflow.frontmatter().tracker().success_state(),
		project.codex().review_level(),
	)
}

fn maybe_finalize_after_terminalized_app_server_failure<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	tracker_tool_bridge: &TrackerToolBridge<'_>,
	error: &Report,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	let Some(disposition) = tracker_tool_bridge.finalized_completion_disposition()? else {
		return Ok(None);
	};

	state_store.append_private_execution_event(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.run_id,
		issue_run.attempt_number,
		"terminal_finalize_app_server_failure_recovery",
		serde_json::json!({
			"path": disposition.as_str(),
			"source_error": error.to_string(),
			"recovery": "apply_terminal_completion_writeback",
		}),
	)?;

	tracing::warn!(
		project_id = project.service_id(),
		issue_id = issue_run.issue.id,
		issue = issue_run.issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		path = disposition.as_str(),
		error = %error,
		"App-server run failed after terminal finalize; applying terminal completion writeback."
	);

	apply_run_completion_disposition(
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		tracker_tool_bridge,
	)?;

	state_store.record_run_attempt(
		&issue_run.run_id,
		&issue_run.issue.id,
		issue_run.attempt_number,
		"succeeded",
	)?;

	Ok(Some(run_summary_from_issue_run(project.service_id(), issue_run)))
}

fn build_issue_turn_continuation_guard<'a, T>(
	tracker: &'a T,
	tracker_tool_bridge: &'a TrackerToolBridge<'a>,
	workflow: &'a WorkflowDocument,
	project: &'a ServiceConfig,
	issue_run: &'a IssueRunPlan,
	review_state_inspector: Option<&'a dyn PullRequestReviewStateInspector>,
) -> IssueTurnContinuationGuard<'a, T>
where
	T: IssueTracker,
{
	IssueTurnContinuationGuard {
		tracker,
		tracker_tool_bridge,
		workflow,
		service_id: project.service_id(),
		issue_id: &issue_run.issue.id,
		issue_identifier: &issue_run.issue.identifier,
		initial_issue_state: &issue_run.initial_issue_state,
		#[cfg(test)]
		retry_project_slug: "",
		dispatch_mode: issue_run.dispatch_mode,
		review_state_inspector,
	}
}

fn finalize_completed_app_server_run<T>(run: CompletedAppServerRun<'_, T>) -> Result<RunSummary>
where
	T: IssueTracker,
{
	apply_run_completion_disposition(
		run.tracker,
		run.project,
		run.workflow,
		run.state_store,
		run.issue_run,
		run.tracker_tool_bridge,
	)?;
	archive_completed_issue_threads_best_effort(
		run.project,
		run.state_store,
		run.issue_run,
		run.process_env,
		run.transport,
		run.run_result,
	);

	Ok(run_summary_from_issue_run(run.project.service_id(), run.issue_run))
}

fn archive_completed_issue_threads_best_effort(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	process_env: &AppServerProcessEnv,
	transport: &str,
	run_result: &AppServerRunResult,
) {
	let current = ThreadArchiveCandidate {
		issue_id: issue_run.issue.id.clone(),
		issue_identifier: issue_run.issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		attempt_number: issue_run.attempt_number,
		thread_id: run_result.thread_id.clone(),
		sequence_number: run_result.event_count.saturating_add(1),
	};

	archive_issue_threads_best_effort(
		project,
		state_store,
		&issue_run.issue.id,
		&issue_run.issue.identifier,
		process_env,
		transport,
		Some(current),
	);
}

fn reconcile_terminal_thread_archive_backlog_best_effort(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) {
	let process_env = AppServerProcessEnv::default();
	let transport = workflow.frontmatter().agent().transport();
	let candidates =
		match terminal_thread_archive_backlog_candidates(state_store, project.service_id()) {
			Ok(candidates) => candidates,
			Err(error) => {
				tracing::warn!(
					?error,
					project_id = project.service_id(),
					"Failed to list terminal thread archive backlog; skipping this archive reconciliation pass."
				);

				return;
			},
		};

	for candidate in candidates {
		archive_completed_issue_thread_best_effort(
			project,
			state_store,
			&process_env,
			transport,
			&candidate,
		);
	}
}

fn archive_issue_threads_best_effort(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_id: &str,
	issue_identifier: &str,
	process_env: &AppServerProcessEnv,
	transport: &str,
	current: Option<ThreadArchiveCandidate>,
) {
	let fallback_candidate = current.clone();
	let candidates =
		match issue_thread_archive_candidates(state_store, issue_id, issue_identifier, current) {
			Ok(candidates) => candidates,
			Err(error) => {
				tracing::warn!(
					?error,
					project_id = project.service_id(),
					issue_id,
					issue = issue_identifier,
					"Failed to list completed issue threads for archive; archiving current thread only."
				);

				fallback_candidate.into_iter().collect()
			},
		};

	for candidate in candidates {
		archive_completed_issue_thread_best_effort(
			project,
			state_store,
			process_env,
			transport,
			&candidate,
		);
	}
}

#[cfg(test)]
fn completed_issue_thread_archive_candidates(
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	run_result: &AppServerRunResult,
) -> Result<Vec<ThreadArchiveCandidate>> {
	issue_thread_archive_candidates(
		state_store,
		&issue_run.issue.id,
		&issue_run.issue.identifier,
		Some(ThreadArchiveCandidate {
			issue_id: issue_run.issue.id.clone(),
			issue_identifier: issue_run.issue.identifier.clone(),
			run_id: issue_run.run_id.clone(),
			attempt_number: issue_run.attempt_number,
			thread_id: run_result.thread_id.clone(),
			sequence_number: run_result.event_count.saturating_add(1),
		}),
	)
}

fn issue_thread_archive_candidates(
	state_store: &StateStore,
	issue_id: &str,
	issue_identifier: &str,
	current: Option<ThreadArchiveCandidate>,
) -> Result<Vec<ThreadArchiveCandidate>> {
	let mut seen_thread_ids = HashSet::new();
	let mut candidates = Vec::new();

	if let Some(current) = current {
		push_thread_archive_candidate(
			state_store,
			&mut candidates,
			&mut seen_thread_ids,
			ThreadArchiveCandidateSource {
				run_id: &current.run_id,
				issue_id: &current.issue_id,
				issue_identifier: &current.issue_identifier,
				attempt_number: current.attempt_number,
				thread_id: &current.thread_id,
				sequence_number: Some(current.sequence_number),
			},
		)?;
	}

	for attempt in state_store.list_run_attempts_for_issue(issue_id)? {
		if !completed_issue_archive_attempt_status(attempt.status()) {
			continue;
		}

		if let Some(thread_id) = attempt.thread_id() {
			push_thread_archive_candidate(
				state_store,
				&mut candidates,
				&mut seen_thread_ids,
				ThreadArchiveCandidateSource {
					run_id: attempt.run_id(),
					issue_id: attempt.issue_id(),
					issue_identifier,
					attempt_number: attempt.attempt_number(),
					thread_id,
					sequence_number: None,
				},
			)?;
		}
	}

	Ok(candidates)
}

fn terminal_thread_archive_backlog_candidates(
	state_store: &StateStore,
	project_id: &str,
) -> Result<Vec<ThreadArchiveCandidate>> {
	let mut seen_thread_ids = HashSet::new();
	let mut candidates = Vec::new();

	for attempt in state_store.list_run_attempts_for_project(project_id)? {
		if !completed_issue_archive_attempt_status(attempt.status()) {
			continue;
		}

		if let Some(thread_id) = attempt.thread_id() {
			push_thread_archive_candidate(
				state_store,
				&mut candidates,
				&mut seen_thread_ids,
				ThreadArchiveCandidateSource {
					run_id: attempt.run_id(),
					issue_id: attempt.issue_id(),
					issue_identifier: attempt.issue_id(),
					attempt_number: attempt.attempt_number(),
					thread_id,
					sequence_number: None,
				},
			)?;
		}
	}

	Ok(candidates)
}

fn completed_issue_archive_attempt_status(status: &str) -> bool {
	matches!(
		status,
		"succeeded" | "failed" | "interrupted" | "terminated" | TERMINAL_GUARDED_RUN_STATUS
	)
}

fn push_thread_archive_candidate(
	state_store: &StateStore,
	candidates: &mut Vec<ThreadArchiveCandidate>,
	seen_thread_ids: &mut HashSet<String>,
	source: ThreadArchiveCandidateSource<'_>,
) -> Result<()> {
	if !seen_thread_ids.insert(source.thread_id.to_owned())
		|| run_has_terminal_thread_archive_event(state_store, source.run_id)?
	{
		return Ok(());
	}

	candidates.push(ThreadArchiveCandidate {
		issue_id: source.issue_id.to_owned(),
		issue_identifier: source.issue_identifier.to_owned(),
		run_id: source.run_id.to_owned(),
		attempt_number: source.attempt_number,
		thread_id: source.thread_id.to_owned(),
		sequence_number: source
			.sequence_number
			.unwrap_or(state_store.event_count(source.run_id)?.saturating_add(1)),
	});

	Ok(())
}

fn run_has_terminal_thread_archive_event(state_store: &StateStore, run_id: &str) -> Result<bool> {
	for event_type in ["thread/archive", "thread/archive/discarded"] {
		if state_store.run_has_protocol_event(run_id, event_type)? {
			return Ok(true);
		}
	}

	Ok(false)
}

fn archive_completed_issue_thread_best_effort(
	project: &ServiceConfig,
	state_store: &StateStore,
	process_env: &AppServerProcessEnv,
	transport: &str,
	candidate: &ThreadArchiveCandidate,
) {
	let archive_request = AppServerThreadArchiveRequest {
		run_id: &candidate.run_id,
		issue_id: &candidate.issue_id,
		attempt_number: candidate.attempt_number,
		listen: transport,
		process_env,
		thread_id: &candidate.thread_id,
		sequence_number: candidate.sequence_number,
	};
	#[cfg(not(test))]
	let archive_result = agent::archive_app_server_thread_after_success(&archive_request, state_store);
	#[cfg(test)]
	let archive_result = {
		state_store
			.append_event(
				archive_request.run_id,
				archive_request.sequence_number,
				"thread/archive",
				&serde_json::json!({
					"threadId": archive_request.thread_id,
					"issueId": archive_request.issue_id,
					"attemptNumber": archive_request.attempt_number,
				})
				.to_string(),
			)
			.map(|()| AppServerThreadArchiveOutcome::Archived)
	};

	match archive_result {
		Ok(AppServerThreadArchiveOutcome::Archived) => tracing::info!(
			project_id = project.service_id(),
			issue_id = candidate.issue_id,
			issue = candidate.issue_identifier,
			run_id = candidate.run_id,
			attempt = candidate.attempt_number,
			thread_id = %candidate.thread_id,
			"Archived completed issue app-server thread."
		),
		Ok(AppServerThreadArchiveOutcome::DiscardedMissingThread) => tracing::info!(
			project_id = project.service_id(),
			issue_id = candidate.issue_id,
			issue = candidate.issue_identifier,
			run_id = candidate.run_id,
			attempt = candidate.attempt_number,
			thread_id = %candidate.thread_id,
			"Discarded completed issue app-server thread archive because the thread is missing."
		),
		Err(error) => tracing::warn!(
			?error,
			project_id = project.service_id(),
			issue_id = candidate.issue_id,
			issue = candidate.issue_identifier,
			run_id = candidate.run_id,
			attempt = candidate.attempt_number,
			thread_id = %candidate.thread_id,
			"Failed to archive completed issue app-server thread; leaving completed run intact."
		),
	}
}

fn maybe_execute_deterministic_closeout<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	tracker_tool_bridge: &TrackerToolBridge<'_>,
	review_context: &ReviewHandoffContext,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	if issue_run.dispatch_mode != IssueDispatchMode::Closeout {
		return Ok(None);
	}

	execute_deterministic_closeout(
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		tracker_tool_bridge,
		review_context,
	)?;

	Ok(Some(run_summary_from_issue_run(project.service_id(), issue_run)))
}

fn build_run_developer_instructions<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	review_context: &ReviewHandoffContext,
) -> Result<String>
where
	T: IssueTracker,
{
	build_developer_instructions(
		tracker,
		project,
		workflow,
		issue_run,
		state_store,
		review_context.recorded_pr_url.as_deref(),
	)
}

fn build_run_user_input<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	review_context: &ReviewHandoffContext,
) -> String
where
	T: IssueTracker,
{
	build_user_input(
		tracker,
		project,
		&issue_run.issue,
		workflow,
		issue_run,
		state_store,
		review_context.recorded_pr_url.as_deref(),
	)
}

fn build_decodex_run_context(
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
) -> DecodexRunContext {
	let execution = workflow.frontmatter().execution();

	DecodexRunContext {
		run_id: issue_run.run_id.clone(),
		attempt_number: issue_run.attempt_number,
		issue_id: issue_run.issue.id.clone(),
		issue_identifier: issue_run.issue.identifier.clone(),
		branch: issue_run.worktree.branch_name.clone(),
		worktree_path: issue_run.worktree.path.display().to_string(),
		max_turns: execution.max_turns(),
		default_canonicalize_commands: execution.canonicalize_commands().to_vec(),
		default_verify_commands: execution.verify_commands().to_vec(),
	}
}

fn write_git_credentials_operation_marker(issue_run: &IssueRunPlan) {
	write_run_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_GIT_CREDENTIALS,
	);
}

fn execute_deterministic_closeout<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	tracker_tool_bridge: &TrackerToolBridge<'_>,
	review_context: &ReviewHandoffContext,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	write_run_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_REVIEW_WRITEBACK,
	);

	let pr_url = review_context.recorded_pr_url.as_deref().ok_or_else(|| {
		eyre::eyre!(
			"Retained closeout run `{}` for issue `{}` requires a recorded PR URL.",
			issue_run.run_id,
			issue_run.issue.identifier
		)
	})?;
	let pull_request = tracker_tool_bridge.validate_deterministic_closeout_pr(pr_url)?;
	let cleanup_commit_sha = worktree_head_oid(&issue_run.worktree.path)?;

	ensure_closeout_issue_completed_state(tracker, workflow, issue_run)?;

	tracker_tool_bridge.apply_validated_deterministic_closeout(pull_request)?;

	cleanup_completed_post_review_lane(project, workflow, state_store, issue_run)?;
	write_cleanup_complete_lifecycle_event(
		tracker,
		project,
		state_store,
		issue_run,
		Some(pr_url),
		cleanup_commit_sha.as_deref(),
	)?;

	tracker_tool_bridge.clear_closeout_issue_scope()?;

	Ok(())
}

fn ensure_closeout_issue_completed_state<T>(
	tracker: &T,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();
	let completed_state = tracker_policy.resolved_completed_state();
	let mut refreshed_issues = tracker.refresh_issues(slice::from_ref(&issue_run.issue.id))?;
	let current_issue = refreshed_issues.pop().unwrap_or_else(|| issue_run.issue.clone());

	if current_issue.state.name == completed_state {
		return Ok(());
	}
	if current_issue.state.name != tracker_policy.success_state() {
		eyre::bail!(
			"Retained closeout for issue `{}` requires tracker state `{}` or `{}`, but the refreshed issue is `{}`.",
			current_issue.identifier,
			tracker_policy.success_state(),
			completed_state,
			current_issue.state.name
		);
	}

	let state_id = current_issue.state_id_for_name(completed_state).ok_or_else(|| {
		eyre::eyre!(
			"Issue `{}` does not expose tracker state `{}` on its team.",
			current_issue.identifier,
			completed_state
		)
	})?;

	tracker.update_issue_state(&current_issue.id, state_id)?;

	Ok(())
}

fn run_completion_repo_gate(workflow: &WorkflowDocument, issue_run: &IssueRunPlan) -> Result<()> {
	let selected_repo_gate =
		select_repo_gate_for_worktree(workflow.frontmatter().execution(), &issue_run.worktree.path);

	write_run_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_REPO_GATE,
	);
	run_repo_gate_commands(
		selected_repo_gate.canonicalize_commands(),
		selected_repo_gate.verify_commands(),
		&issue_run.worktree.path,
	)?;
	write_run_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_REVIEW_WRITEBACK,
	);

	Ok(())
}

fn push_retained_review_repair_head(
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
	pr_url: Option<&str>,
) -> Result<()> {
	let token_env_var = project.github().token_env_var();
	let github_token = resolve_configured_env_var("github.token_env_var", Some(token_env_var))
		.map_err(|error| {
			Report::new(RetainedReviewRepairPushFailed {
				issue_identifier: issue_run.issue.identifier.clone(),
				run_id: issue_run.run_id.clone(),
				branch_name: issue_run.worktree.branch_name.clone(),
				pr_url: pr_url.map(ToOwned::to_owned),
				kind: RetainedReviewRepairPushFailureKind::Auth,
				detail: error.to_string(),
			})
		})?;
	let git_credentials =
		GitCredentialSource::new(token_env_var, &github_token).materialize_github_credentials();
	let refspec = format!("HEAD:{}", issue_run.worktree.branch_name);
	let mut command = Command::new("git");

	command.arg("-C").arg(&issue_run.worktree.path).arg("push").arg("origin").arg(&refspec);
	git_credentials.apply_to(&mut command);

	let output = command.output().map_err(|error| {
		Report::new(RetainedReviewRepairPushFailed {
			issue_identifier: issue_run.issue.identifier.clone(),
			run_id: issue_run.run_id.clone(),
			branch_name: issue_run.worktree.branch_name.clone(),
			pr_url: pr_url.map(ToOwned::to_owned),
			kind: RetainedReviewRepairPushFailureKind::Failed,
			detail: error.to_string(),
		})
	})?;

	if output.status.success() {
		return Ok(());
	}

	let detail = repo_gate_output_text(&output);
	let kind = classify_retained_review_repair_push_failure(&detail);

	Err(Report::new(RetainedReviewRepairPushFailed {
		issue_identifier: issue_run.issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		branch_name: issue_run.worktree.branch_name.clone(),
		pr_url: pr_url.map(ToOwned::to_owned),
		kind,
		detail,
	}))
}

fn classify_retained_review_repair_push_failure(
	detail: &str,
) -> RetainedReviewRepairPushFailureKind {
	let normalized = detail.to_ascii_lowercase();

	if normalized.contains("authentication failed")
		|| normalized.contains("could not read username")
		|| normalized.contains("permission denied")
		|| normalized.contains("repository not found")
		|| normalized.contains("403")
		|| normalized.contains("401")
	{
		return RetainedReviewRepairPushFailureKind::Auth;
	}
	if normalized.contains("src refspec")
		|| normalized.contains("dst refspec")
		|| normalized.contains("invalid refspec")
	{
		return RetainedReviewRepairPushFailureKind::Refspec;
	}
	if normalized.contains("rejected")
		|| normalized.contains("non-fast-forward")
		|| normalized.contains("fetch first")
		|| normalized.contains("protected branch hook declined")
	{
		return RetainedReviewRepairPushFailureKind::RemoteRejected;
	}

	RetainedReviewRepairPushFailureKind::Failed
}

fn apply_run_completion_disposition<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	tracker_tool_bridge: &TrackerToolBridge<'_>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	match tracker_tool_bridge.completion_disposition()? {
		RunCompletionDisposition::ReviewHandoff => {
			validate_review_handoff_runtime(project, false)?;
			run_completion_repo_gate(workflow, issue_run)?;

			tracker_tool_bridge.apply_review_handoff().map_err(|error| {
				if let Some(writeback_error) = error.downcast_ref::<ReviewHandoffWritebackFailed>()
				{
					Report::new(ReviewHandoffNeedsAttention {
						issue_identifier: writeback_error.issue_identifier.clone(),
						pr_url: writeback_error.pr_url.clone(),
						run_id: writeback_error.run_id.clone(),
					})
					.wrap_err(error)
				} else {
					error
				}
			})?;

			record_harness_outcome_best_effort(
				state_store,
				project.service_id(),
				issue_run,
				HarnessOutcomeKind::ReviewHandoff,
				None,
				Some("passed"),
				tracker_tool_bridge
					.review_context()
					.and_then(|context| context.recorded_pr_url.as_deref()),
			);
		},
		RunCompletionDisposition::ManualAttention => {
			return Err(Report::new(ManualAttentionRequested {
				issue_identifier: issue_run.issue.identifier.clone(),
				label: workflow.frontmatter().tracker().needs_attention_label().to_owned(),
				run_id: issue_run.run_id.clone(),
				error_class: tracker_tool_bridge.manual_attention_error_class(),
			}));
		},
		RunCompletionDisposition::ReviewRepair => {
			validate_review_repair_runtime(project, false)?;
			run_completion_repo_gate(workflow, issue_run)?;
			push_retained_review_repair_head(
				project,
				issue_run,
				tracker_tool_bridge
					.review_context()
					.and_then(|context| context.recorded_pr_url.as_deref()),
			)?;

			tracker_tool_bridge.apply_review_repair()?;

			record_harness_outcome_best_effort(
				state_store,
				project.service_id(),
				issue_run,
				HarnessOutcomeKind::ReviewRepair,
				None,
				Some("passed"),
				tracker_tool_bridge
					.review_context()
					.and_then(|context| context.recorded_pr_url.as_deref()),
			);
		},
		RunCompletionDisposition::Closeout => {
			write_run_operation_marker_best_effort(
				&issue_run.worktree.path,
				&issue_run.run_id,
				issue_run.attempt_number,
				RUN_OPERATION_REVIEW_WRITEBACK,
			);

			let cleanup_commit_sha = worktree_head_oid(&issue_run.worktree.path)?;

			tracker_tool_bridge.apply_closeout()?;

			cleanup_completed_post_review_lane(project, workflow, state_store, issue_run)?;
			write_cleanup_complete_lifecycle_event(
				tracker,
				project,
				state_store,
				issue_run,
				tracker_tool_bridge
					.review_context()
					.and_then(|context| context.recorded_pr_url.as_deref()),
				cleanup_commit_sha.as_deref(),
			)?;

			tracker_tool_bridge.clear_closeout_issue_scope()?;

			record_harness_outcome_best_effort(
				state_store,
				project.service_id(),
				issue_run,
				HarnessOutcomeKind::Closeout,
				None,
				Some("passed"),
				tracker_tool_bridge
					.review_context()
					.and_then(|context| context.recorded_pr_url.as_deref()),
			);
		},
	}

	Ok(())
}

fn write_run_operation_marker_best_effort(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	current_operation: &str,
) {
	if let Err(error) =
		state::write_run_operation_marker(worktree_path, run_id, attempt_number, current_operation)
	{
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			current_operation,
			worktree_path = %worktree_path.display(),
			"Run operation marker write failed; continuing completion flow."
		);
	}
}

fn run_summary_from_issue_run(project_id: &str, issue_run: &IssueRunPlan) -> RunSummary {
	RunSummary {
		project_id: project_id.to_owned(),
		issue_id: issue_run.issue.id.clone(),
		issue_identifier: issue_run.issue.identifier.clone(),
		issue_state: issue_run.issue_state.clone(),
		initial_issue_state: issue_run.initial_issue_state.clone(),
		#[cfg(test)]
		retry_project_slug: String::new(),
		dispatch_mode: issue_run.dispatch_mode,
		branch_name: issue_run.worktree.branch_name.clone(),
		worktree_path: issue_run.worktree.path.clone(),
		attempt_number: issue_run.attempt_number,
		run_id: issue_run.run_id.clone(),
		continuation_pending: false,
	}
}

fn continuation_boundary_summary(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	run_result: &AppServerRunResult,
) -> RunSummary {
	tracing::info!(
		project_id = project.service_id(),
		issue_id = issue_run.issue.id,
		issue = issue_run.issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		thread_id = run_result.thread_id,
		turn_count = run_result.turn_count,
		max_turns = workflow.frontmatter().execution().max_turns(),
		"Run reached a clean continuation boundary and will rely on the next bounded re-entry."
	);

	RunSummary {
		continuation_pending: true,
		..run_summary_from_issue_run(project.service_id(), issue_run)
	}
}

fn planned_issue_state_for_dispatch(
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
	dispatch_mode: IssueDispatchMode,
	preferred_issue_state: Option<&str>,
) -> String {
	match dispatch_mode {
		IssueDispatchMode::Normal | IssueDispatchMode::Program =>
			workflow.frontmatter().tracker().in_progress_state().to_owned(),
		IssueDispatchMode::Retry => preferred_issue_state
			.filter(|state| {
				*state == workflow.frontmatter().tracker().in_progress_state()
					&& workflow
						.frontmatter()
						.tracker()
						.startable_states()
						.iter()
						.any(|candidate| candidate == &issue.state.name)
			})
			.map(|_| {
				preferred_issue_state
					.expect("filtered preferred issue state should exist")
					.to_owned()
			})
			.unwrap_or_else(|| issue.state.name.clone()),
		IssueDispatchMode::ReviewRepair | IssueDispatchMode::Closeout => issue.state.name.clone(),
	}
}
