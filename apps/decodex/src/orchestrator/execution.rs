use git_credentials::GitSigningConfig;
use agent::CodexAccountPool;
use agent::CodexAccountProvider;

struct AgentGitCredentialEnvironment {
	process_env: AppServerProcessEnv,
	askpass_path: PathBuf,
}
impl AgentGitCredentialEnvironment {
	fn process_env(&self) -> &AppServerProcessEnv {
		&self.process_env
	}
}

impl Drop for AgentGitCredentialEnvironment {
	fn drop(&mut self) {
		if let Err(error) = fs::remove_file(&self.askpass_path)
			&& error.kind() != ErrorKind::NotFound
		{
			tracing::warn!(
				?error,
				askpass_path = %self.askpass_path.display(),
				"Failed to remove agent Git askpass helper."
			);
		}
	}
}

struct TerminalFailureLifecycle<'a> {
	error_class: &'a str,
	next_action: &'a str,
	target_state: &'a str,
	worktree_path: &'a str,
	manual_attention_requested: bool,
}

struct TerminalFailureWritebackRuntime<'a> {
	service_id: &'a str,
	state_store: Option<&'a StateStore>,
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

	let result = ensure_automation_activity_label(tracker, &issue_run.issue, project.service_id(), true)
		.and_then(|_| execute_issue_run_inner(tracker, project, workflow, state_store, &issue_run));

	state_store.clear_lease(&issue_run.issue.id)?;

	match result {
		Ok(summary) => {
			persist_issue_run_outcome(state_store, &issue_run.run_id, &summary)?;

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
	let askpass_path = agent_git_askpass_path(project.worktree_root(), run_id);
	let signing_config = GitSigningConfig::from_local_git_config(worktree_path)?;

	git_credentials::write_github_askpass_helper(&askpass_path)?;

	Ok(AgentGitCredentialEnvironment {
		process_env: AppServerProcessEnv::with_github_credentials_and_signing_config(
			project.github().token_env_var().to_owned(),
			github_token,
			askpass_path.clone(),
			signing_config,
		),
		askpass_path,
	})
}

fn agent_git_askpass_path(worktree_root: &Path, run_id: &str) -> PathBuf {
	let safe_run_id = sanitize_run_id_for_path(run_id);

	worktree_root.join(format!("{AGENT_GIT_ASKPASS_PREFIX}{safe_run_id}.sh"))
}

fn sanitize_run_id_for_path(run_id: &str) -> String {
	let sanitized = run_id
		.chars()
		.map(|character| {
			if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
				character
			} else {
				'_'
			}
		})
		.collect::<String>();

	if sanitized.is_empty() { String::from("run") } else { sanitized }
}

fn lifecycle_event_identity<'a>(
	project: &'a ServiceConfig,
	issue_run: &'a IssueRunPlan,
) -> records::LinearExecutionEventIdentity<'a> {
	records::LinearExecutionEventIdentity {
		service_id: project.service_id(),
		issue_id: &issue_run.issue.id,
		issue_identifier: &issue_run.issue.identifier,
		run_id: &issue_run.run_id,
		attempt_number: issue_run.attempt_number,
	}
}

fn write_lifecycle_event<T>(
	tracker: &T,
	state_store: &StateStore,
	issue_id: &str,
	record: &records::LinearExecutionEventRecord,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let body = format!("Decodex execution event: {}", record.event_type);

	if state_store.record_linear_execution_event(record)?
		&& let Err(error) = tracker::create_linear_execution_event_comment_without_remote_scan(
			tracker, issue_id, &body, record,
		)
	{
		state_store.forget_linear_execution_event(&record.idempotency_key)?;

		return Err(error);
	}

	Ok(())
}

fn write_prepare_lifecycle_events<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let worktree_path = relative_worktree_path(project, &issue_run.worktree);
	let commit_sha = worktree_head_oid(&issue_run.worktree.path)?.ok_or_else(|| {
		eyre::eyre!(
			"Prepared worktree `{}` for issue `{}` did not expose a HEAD commit.",
			issue_run.worktree.path.display(),
			issue_run.issue.identifier
			)
		})?;

	write_run_started_lifecycle_event(
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		&worktree_path,
		&commit_sha,
	)
}

fn write_run_started_lifecycle_event<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	worktree_path: &str,
	commit_sha: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let transport = workflow.frontmatter().agent().transport();
	let anchor = records::stable_event_anchor(&[
		issue_run.dispatch_mode.as_str(),
		&issue_run.worktree.branch_name,
		commit_sha,
		transport,
	]);
	let mut record = records::LinearExecutionEventRecord::new(
		lifecycle_event_identity(project, issue_run),
		"run_started",
		current_timestamp(),
		&anchor,
	);

	record.branch = Some(issue_run.worktree.branch_name.clone());
	record.worktree_path = Some(worktree_path.to_owned());
	record.commit_sha = Some(commit_sha.to_owned());
	record.transport = Some(transport.to_owned());
	record.summary = Some(format!(
		"Decodex started a {} run for this issue.",
		issue_run.dispatch_mode.as_str()
	));

	write_lifecycle_event(tracker, state_store, &issue_run.issue.id, &record)
}

fn terminal_failure_lifecycle_event(
	service_id: &str,
	issue_run: &IssueRunPlan,
	failure: TerminalFailureLifecycle<'_>,
) -> records::LinearExecutionEventRecord {
	let event_type = if failure.manual_attention_requested {
		"needs_attention"
	} else {
		"terminal_failure"
	};
	let anchor = records::stable_event_anchor(&[
		event_type,
		failure.error_class,
		failure.target_state,
	]);
	let mut record = records::LinearExecutionEventRecord::new(
		records::LinearExecutionEventIdentity {
			service_id,
			issue_id: &issue_run.issue.id,
			issue_identifier: &issue_run.issue.identifier,
			run_id: &issue_run.run_id,
			attempt_number: issue_run.attempt_number,
		},
		event_type,
		current_timestamp(),
		&anchor,
	);

	record.branch = Some(issue_run.worktree.branch_name.clone());
	record.worktree_path = Some(failure.worktree_path.to_owned());
	record.error_class = Some(failure.error_class.to_owned());
	record.next_action = Some(failure.next_action.to_owned());
	record.blockers = Some(vec![format!("Run failed with `{}`.", failure.error_class)]);
	record.evidence = Some(vec![format!(
		"Attempt {} reached terminal failure handling.",
		issue_run.attempt_number
	)]);
	record.summary = Some(String::from("Decodex run failed and needs attention."));
	record.target_state = Some(failure.target_state.to_owned());

	if failure.manual_attention_requested {
		record.terminal_path = Some(String::from("manual_attention"));
	}

	record
}

fn write_cleanup_complete_lifecycle_event<T>(
	tracker: &T,
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	pr_url: Option<&str>,
	commit_sha: Option<&str>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let worktree_path = relative_worktree_path(project, &issue_run.worktree);
	let anchor = records::stable_event_anchor(&[
		&issue_run.worktree.branch_name,
		commit_sha.unwrap_or_default(),
		"cleanup_complete",
	]);
	let mut record = records::LinearExecutionEventRecord::new(
		lifecycle_event_identity(project, issue_run),
		"cleanup_complete",
		current_timestamp(),
		&anchor,
	);

	record.branch = Some(issue_run.worktree.branch_name.clone());
	record.worktree_path = Some(worktree_path);
	record.cleanup_status = Some(String::from("completed"));
	record.summary = Some(String::from("Decodex cleaned up the retained lane worktree."));
	record.pr_url = pr_url.map(ToOwned::to_owned);
	record.commit_sha = commit_sha.map(ToOwned::to_owned);

	write_lifecycle_event(tracker, state_store, &issue_run.issue.id, &record)
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
	let review_context = build_review_run_context(project, state_store, issue_run)?;
	let tracker_tool_bridge = TrackerToolBridge::with_run_context_and_state_store(
		tracker,
		&issue_run.issue,
		workflow,
		review_context.clone(),
		state_store,
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
	let closeout_review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
	};
	let continuation_guard = IssueTurnContinuationGuard {
		tracker,
		tracker_tool_bridge: &tracker_tool_bridge,
		workflow,
		service_id: project.service_id(),
		issue_id: &issue_run.issue.id,
		issue_identifier: &issue_run.issue.identifier,
		initial_issue_state: &issue_run.initial_issue_state,
		#[cfg(test)]
		retry_project_slug: "",
		dispatch_mode: issue_run.dispatch_mode,
		review_state_inspector: Some(&closeout_review_state_inspector),
	};
	let decodex_tool_bridge =
		DecodexToolBridge::new(&tracker_tool_bridge, build_decodex_run_context(workflow, issue_run));
	let run_result = agent::execute_app_server_run(
		&AppServerRunRequest {
			run_id: issue_run.run_id.clone(),
			issue_id: issue_run.issue.id.clone(),
			attempt_number: issue_run.attempt_number,
			listen: transport,
			cwd: issue_run.worktree.path.display().to_string(),
			developer_instructions: build_run_developer_instructions(
				tracker,
				project,
				workflow,
				state_store,
				issue_run,
				&review_context,
			)?,
			user_input: build_run_user_input(
				tracker,
				project,
				workflow,
				state_store,
				issue_run,
				&review_context,
			),
			max_turns: workflow.frontmatter().execution().max_turns(),
			timeout: ACTIVE_RUN_IDLE_TIMEOUT,
			process_env: agent_git_credentials.process_env().clone(),
			continuation_user_input: Some(build_continuation_user_input(
				&issue_run.issue,
				workflow,
				issue_run.dispatch_mode,
				review_context.recorded_pr_url.as_deref(),
				workflow.frontmatter().tracker().success_state(),
				project.codex().internal_review_mode(),
			)),
			activity_marker_path: Some(issue_run.worktree.path.clone()),
			resume_thread_id: resolve_resume_thread_id(state_store, issue_run)?,
			ephemeral_thread: false,
			command_exec_health_check: None,
			dynamic_tool_handler: Some(&decodex_tool_bridge),
			continuation_guard: Some(&continuation_guard),
			codex_account_provider: codex_account_pool
				.as_ref()
				.map(|pool| pool as &dyn CodexAccountProvider),
		},
		state_store,
	)
	.map_err(|error| {
		preserve_manual_attention_request(
			tracker_tool_bridge.completion_disposition(),
			issue_run,
			workflow,
			error,
		)
	})?;

	if run_result.continuation_pending {
		return Ok(continuation_boundary_summary(project, workflow, issue_run, &run_result));
	}

	apply_run_completion_disposition(
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		&tracker_tool_bridge,
	)?;

	Ok(run_summary_from_issue_run(project.service_id(), issue_run))
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

			tracker_tool_bridge.apply_review_handoff().map_err(|error| {
				if let Some(writeback_error) = error.downcast_ref::<ReviewHandoffWritebackFailed>()
				{
					Report::new(ReviewHandoffNeedsAttention {
						issue_identifier: writeback_error.issue_identifier.clone(),
						run_id: writeback_error.run_id.clone(),
					})
					.wrap_err(error)
				} else {
					error
				}
			})?;
		},
		RunCompletionDisposition::ManualAttention => {
			return Err(Report::new(ManualAttentionRequested {
				issue_identifier: issue_run.issue.identifier.clone(),
				label: workflow.frontmatter().tracker().needs_attention_label().to_owned(),
				run_id: issue_run.run_id.clone(),
			}));
		},
		RunCompletionDisposition::ReviewRepair => {
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

			tracker_tool_bridge.apply_review_repair()?;
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
	if let Err(error) = state::write_run_operation_marker(
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

	RunSummary { continuation_pending: true, ..run_summary_from_issue_run(project.service_id(), issue_run) }
}

fn planned_issue_state_for_dispatch(
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
	dispatch_mode: IssueDispatchMode,
	preferred_issue_state: Option<&str>,
) -> String {
	match dispatch_mode {
		IssueDispatchMode::Normal =>
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
		IssueDispatchMode::ReviewRepair | IssueDispatchMode::Closeout =>
			issue.state.name.clone(),
	}
}

fn preserve_manual_attention_request(
	completion_disposition: Result<RunCompletionDisposition>,
	issue_run: &IssueRunPlan,
	workflow: &WorkflowDocument,
	error: Report,
) -> Report {
	if matches!(completion_disposition, Ok(RunCompletionDisposition::ManualAttention)) {
		return Report::new(ManualAttentionRequested {
			issue_identifier: issue_run.issue.identifier.clone(),
			label: workflow.frontmatter().tracker().needs_attention_label().to_owned(),
			run_id: issue_run.run_id.clone(),
		})
		.wrap_err(error);
	}

	error
}

fn run_failure_requires_terminal_attention(error: &Report) -> bool {
	error.downcast_ref::<ManualAttentionRequested>().is_some()
		|| error.downcast_ref::<ReviewHandoffNeedsAttention>().is_some()
		|| error.downcast_ref::<StalledRunNeedsAttention>().is_some()
		|| error.downcast_ref::<AppServerCapabilityPreflightFailure>().is_some()
		|| error.downcast_ref::<AppServerHomePreflightFailure>().is_some()
		|| error.downcast_ref::<AppServerTransportFailure>().is_some()
		|| error.downcast_ref::<AgentGitCredentialsUnavailable>().is_some()
		|| error
			.downcast_ref::<AppServerTurnFailure>()
			.is_some_and(AppServerTurnFailure::requires_operator_attention)
		|| error.downcast_ref::<ReviewPolicyStopRequested>().is_some()
		|| error
			.downcast_ref::<RepoGateFailure>()
			.is_some_and(|repo_gate_failure| {
				repo_gate_failure.disposition() == RepoGateFailureDisposition::NeedsHumanAttention
			})
}

fn handle_failure<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<()>
where
	T: IssueTracker,
{
	let max_attempts = i64::from(workflow.frontmatter().execution().max_attempts());
	let manual_attention_requested = error.downcast_ref::<ManualAttentionRequested>().is_some();
	let requires_terminal_attention = run_failure_requires_terminal_attention(error);
	let worktree_path = relative_worktree_path(project, &issue_run.worktree);
	let retry_budget_attempts =
		retry_budget_attempts_for_current_failure(state_store, issue_run)?;

	if !requires_terminal_attention && retry_budget_attempts < max_attempts {
		let (retry_error_class, retry_next_action) = retry_comment_details(error);

		write_retry_schedule_marker_for_runtime_retry(
			error,
			workflow,
			issue_run,
			retry_budget_attempts,
		)?;

		tracing::warn!(
			project_id = project.service_id(),
			issue_id = issue_run.issue.id,
			issue = issue_run.issue.identifier,
			run_id = issue_run.run_id,
			attempt = issue_run.attempt_number,
			retry_budget_attempt = retry_budget_attempts,
			max_attempts,
			branch = issue_run.worktree.branch_name,
			worktree_path = %worktree_path,
			error_class = retry_error_class,
			"Run failed and remains retryable."
		);

		tracker.create_comment(
			&issue_run.issue.id,
			&format_retry_comment(RetryComment {
				run_id: &issue_run.run_id,
				attempt_number: issue_run.attempt_number,
				retry_budget_attempt_number: retry_budget_attempts,
				max_attempts,
				worktree_path,
				branch_name: &issue_run.worktree.branch_name,
				error_class: retry_error_class,
				next_action: &retry_next_action,
			}),
		)?;

		write_retry_budget_marker(
			&issue_run.worktree.path,
			&issue_run.run_id,
			issue_run.attempt_number,
			retry_budget_attempts,
		)?;

		return Ok(());
	}

	let retained_partial_progress = retained_partial_progress_error(
		error,
		issue_run,
		&worktree_path,
	);
	let terminal_error = retained_partial_progress.as_ref().unwrap_or(error);
	let outcome = apply_terminal_failure_writeback(
		tracker,
		TerminalFailureWritebackRuntime {
			service_id: project.service_id(),
			state_store: Some(state_store),
		},
		workflow,
		issue_run,
		&worktree_path,
		manual_attention_requested,
		terminal_error,
	)?;

	if outcome.retry_guarded_by_state {
		write_terminal_guard_marker(
			&issue_run.worktree.path,
			&issue_run.run_id,
			issue_run.attempt_number,
		)?;

		state_store.update_run_status(&issue_run.run_id, TERMINAL_GUARDED_RUN_STATUS)?;
	}

	write_retry_budget_marker(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = project.service_id(),
		issue_id = issue_run.issue.id,
		issue = issue_run.issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		branch = issue_run.worktree.branch_name,
		worktree_path = %worktree_path,
		error_class = outcome.error_class,
		"Run failed and now requires operator attention."
	);

	Ok(())
}

fn retry_budget_attempts_for_current_failure(
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<i64> {
	let state_attempts = state_store.retry_budget_attempt_count(&issue_run.issue.id)?;
	let current_attempt_counts = state_store
		.run_attempt(&issue_run.run_id)?
		.is_some_and(|attempt| {
			attempt.issue_id() == issue_run.issue.id
				&& matches!(
					attempt.status(),
					"failed" | "interrupted" | "terminal_guarded"
				)
		});
	let previous_state_attempts =
		state_attempts.saturating_sub(i64::from(current_attempt_counts));

	Ok(issue_run.retry_budget_base.max(previous_state_attempts)
		+ i64::from(current_attempt_counts))
}

fn retained_partial_progress_error(
	error: &Report,
	issue_run: &IssueRunPlan,
	worktree_path: &str,
) -> Option<Report> {
	if terminal_failure_has_specific_error_class(error)
		|| !worktree_has_tracked_changes(&issue_run.worktree.path)
	{
		return None;
	}

	Some(Report::new(RetainedPartialProgress {
		issue_identifier: issue_run.issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		worktree_path: worktree_path.to_owned(),
	}))
}

fn terminal_failure_has_specific_error_class(error: &Report) -> bool {
	error.downcast_ref::<ManualAttentionRequested>().is_some()
		|| error.downcast_ref::<ReviewHandoffNeedsAttention>().is_some()
		|| error.downcast_ref::<StalledRunNeedsAttention>().is_some()
		|| error.downcast_ref::<AgentGitCredentialsUnavailable>().is_some()
		|| error.downcast_ref::<AppServerCapabilityPreflightFailure>().is_some()
		|| error.downcast_ref::<AppServerHomePreflightFailure>().is_some()
		|| error.downcast_ref::<AppServerTransportFailure>().is_some()
		|| error.downcast_ref::<AppServerTurnFailure>().is_some()
		|| error.downcast_ref::<ReviewPolicyStopRequested>().is_some()
		|| error.downcast_ref::<RepoGateFailure>().is_some()
}

fn write_retry_schedule_marker_for_runtime_retry(
	error: &Report,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	retry_budget_attempts: i64,
) -> Result<()> {
	let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() else {
		return Ok(());
	};
	let Some(retry_kind) = repo_gate_failure.retry_schedule_kind() else {
		return Ok(());
	};
	let retry_attempt = u32::try_from(retry_budget_attempts).unwrap_or(u32::MAX).max(1);
	let delay = retry_delay(RetryKind::Failure, retry_attempt, workflow);
	let retry_ready_at_unix_epoch = OffsetDateTime::now_utc().unix_timestamp().saturating_add(
		i64::try_from((delay.as_millis().saturating_add(999)) / 1_000).unwrap_or(i64::MAX),
	);

	state::write_run_retry_schedule(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		retry_kind,
		retry_ready_at_unix_epoch,
	)
}

fn apply_terminal_failure_writeback<T>(
	tracker: &T,
	runtime: TerminalFailureWritebackRuntime<'_>,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	worktree_path: &str,
	manual_attention_requested: bool,
	error: &Report,
) -> Result<TerminalFailureOutcome>
where
	T: IssueTracker,
{
	let tracker_policy = workflow.frontmatter().tracker();
	let needs_attention_label = tracker_policy.needs_attention_label();
	let needs_attention_label_id = tracker::issue_team_label_id_with_server_confirmation(
		tracker,
		&issue_run.issue,
		needs_attention_label,
	)?;
	let failure_state_name = tracker_policy.failure_state();
	let failure_state_is_startable =
		tracker_policy.startable_states().iter().any(|state| state == failure_state_name);
	let guard_with_nonstartable_state =
		needs_attention_label_id.is_none() && failure_state_is_startable;
	let terminal_failure_state_name = if guard_with_nonstartable_state {
		tracker_policy.in_progress_state()
	} else {
		failure_state_name
	};
	let failure_state_id =
		issue_run.issue.state_id_for_name(terminal_failure_state_name).ok_or_else(|| {
			eyre::eyre!(
				"State `{}` was not found for issue `{}`.",
				terminal_failure_state_name,
				issue_run.issue.identifier
			)
		})?;

	tracker.update_issue_state(&issue_run.issue.id, failure_state_id)?;

	let needs_attention_label_available = apply_needs_attention_label(
		tracker,
		issue_run,
		runtime.service_id,
		needs_attention_label,
		needs_attention_label_id,
		terminal_failure_state_name,
	)?;
	let recovery_gate = terminal_failure_recovery_gate(
		needs_attention_label,
		needs_attention_label_available,
		guard_with_nonstartable_state,
		tracker_policy.in_progress_state(),
	);
	let (error_class, next_action) =
		terminal_failure_comment_details(manual_attention_requested, error, &recovery_gate);
	let comment = format_terminal_failure_comment(
		&issue_run.run_id,
		issue_run.attempt_number,
		worktree_path.to_owned(),
		&issue_run.worktree.branch_name,
		error_class,
		&next_action,
	);
	let event = terminal_failure_lifecycle_event(
		runtime.service_id,
		issue_run,
		TerminalFailureLifecycle {
			error_class,
			next_action: &next_action,
			target_state: terminal_failure_state_name,
			worktree_path,
			manual_attention_requested,
		},
	);

	if let Some(state_store) = runtime.state_store {
		if state_store.record_linear_execution_event(&event)?
			&& let Err(error) = tracker::create_linear_execution_event_comment_without_remote_scan(
				tracker,
				&issue_run.issue.id,
				&comment,
				&event,
			)
		{
			state_store.forget_linear_execution_event(&event.idempotency_key)?;

			return Err(error);
		}
	} else {
		tracker::create_linear_execution_event_comment(
			tracker,
			&issue_run.issue.id,
			&comment,
			&event,
		)?;
	}

	Ok(TerminalFailureOutcome {
		error_class,
		retry_guarded_by_state: guard_with_nonstartable_state,
	})
}

fn apply_needs_attention_label<T>(
	tracker: &T,
	issue_run: &IssueRunPlan,
	service_id: &str,
	needs_attention_label: &str,
	needs_attention_label_id: Option<String>,
	terminal_failure_state_name: &str,
) -> Result<bool>
where
	T: IssueTracker,
{
	if let Some(label_id) = needs_attention_label_id.as_deref() {
		if !tracker::issue_has_label_with_server_confirmation(
			tracker,
			&issue_run.issue,
			needs_attention_label,
		)? {
			tracker.add_issue_labels(&issue_run.issue.id, &[label_id.to_owned()])?;
		}
	} else {
		tracing::warn!(
			label = needs_attention_label,
			issue = issue_run.issue.identifier,
			guard_state = terminal_failure_state_name,
			"Needs-attention label was not found in the issue team; using a non-startable state guard when needed."
		);
	}

	ensure_automation_activity_label(tracker, &issue_run.issue, service_id, false)?;

	Ok(needs_attention_label_id.is_some())
}

fn ensure_automation_activity_label<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
	present: bool,
) -> Result<()>
where
	T: IssueTracker,
{
	let mut refreshed_issues = tracker.refresh_issues(slice::from_ref(&issue.id))?;
	let current_issue = refreshed_issues.pop().unwrap_or_else(|| issue.clone());
	let active_label = tracker::automation_active_label(service_id);

	tracker::set_issue_label_presence(tracker, &current_issue, &active_label, present)?;

	Ok(())
}
