use color_eyre::Report;

use crate::{
	agent::{
		self, AppServerProcessEnv, AppServerRunRequest, AppServerRunResult, CodexAccountProvider,
	},
	orchestrator::{
		DecodexToolBridge, IssueRunPlan, IssueTracker, IssueTurnContinuationGuard,
		PhaseGoalController, PullRequestReviewStateInspector, Result, ReviewHandoffContext,
		RUN_LEASE_IDLE_TIMEOUT, RunSummary, ServiceConfig, StateStore, TrackerToolBridge,
		TurnContinuationGuard, WorkflowDocument, archive_completed_issue_threads_best_effort,
		build_continuation_user_input, maybe_continue_after_phase_goal_recovery,
		preserve_and_promote_app_server_run_failure, resolve_resume_thread_id,
		run_summary_from_issue_run,
	},
	orchestrator::execution::{
		completion::apply_run_completion_disposition,
		context,
	},
};

pub(super) struct CompletedAppServerRun<'a, T>
where
	T: IssueTracker,
{
	pub(super) tracker: &'a T,
	pub(super) project: &'a ServiceConfig,
	pub(super) workflow: &'a WorkflowDocument,
	pub(super) state_store: &'a StateStore,
	pub(super) issue_run: &'a IssueRunPlan,
	pub(super) tracker_tool_bridge: &'a TrackerToolBridge<'a>,
	pub(super) process_env: &'a AppServerProcessEnv,
	pub(super) transport: &'a str,
	pub(super) run_result: &'a AppServerRunResult,
}

pub(super) struct IssueAppServerRun<'a, T>
where
	T: IssueTracker,
{
	pub(super) tracker: &'a T,
	pub(super) project: &'a ServiceConfig,
	pub(super) workflow: &'a WorkflowDocument,
	pub(super) state_store: &'a StateStore,
	pub(super) issue_run: &'a IssueRunPlan,
	pub(super) tracker_tool_bridge: &'a TrackerToolBridge<'a>,
	pub(super) review_context: &'a ReviewHandoffContext,
	pub(super) process_env: &'a AppServerProcessEnv,
	pub(super) transport: &'a str,
	pub(super) continuation_guard: &'a dyn TurnContinuationGuard,
	pub(super) decodex_tool_bridge: &'a DecodexToolBridge<'a>,
	pub(super) phase_goal_controller: &'a dyn PhaseGoalController,
	pub(super) codex_account_provider: Option<&'a dyn CodexAccountProvider>,
}

pub(super) enum IssueAppServerRunOutcome {
	Completed(AppServerRunResult),
	Finalized(RunSummary),
}

pub(super) fn execute_issue_app_server_run<T>(
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
			developer_instructions: context::build_run_developer_instructions(
				input.tracker,
				input.project,
				input.workflow,
				input.state_store,
				input.issue_run,
				input.review_context,
			)?,
			user_input: context::build_run_user_input(
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

pub(super) fn build_issue_turn_continuation_guard<'a, T>(
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

pub(super) fn finalize_completed_app_server_run<T>(
	run: CompletedAppServerRun<'_, T>,
) -> Result<RunSummary>
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
