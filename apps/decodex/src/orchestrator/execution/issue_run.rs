use orchestrator::execution::app_server;

use crate::{
	agent::{CodexAccountPool, CodexAccountProvider},
	orchestrator::{
		self, DecodexToolBridge, IssueRunPlan, IssueTracker, Result, RunSummary, ServiceConfig,
		StateStore, TrackerToolBridge, WorkflowDocument,
		execution::{
			app_server::{CompletedAppServerRun, IssueAppServerRun, IssueAppServerRunOutcome},
			closeout, context, credentials, resume, runtime_context, summary,
		},
	},
};

pub(crate) fn execute_issue_run<T>(
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
		worktree_path = %orchestrator::relative_worktree_path(project, &issue_run.worktree),
		"Starting issue run."
	);

	state_store.upsert_worktree(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.worktree.branch_name,
		&issue_run.worktree.path.display().to_string(),
	)?;

	let result = orchestrator::ensure_automation_activity_label(
		tracker,
		&issue_run.issue,
		project.service_id(),
		true,
	)
	.and_then(|_| execute_issue_run_inner(tracker, project, workflow, state_store, &issue_run));

	state_store.clear_lease(&issue_run.issue.id)?;

	match result {
		Ok(summary) => {
			resume::persist_issue_run_outcome(state_store, &issue_run.run_id, &summary)?;

			if !summary.continuation_pending {
				state_store.clear_loop_guardrail_checkpoints_for_issue(
					project.service_id(),
					&issue_run.issue.id,
				)?;

				orchestrator::reconcile_terminal_thread_archive_backlog_best_effort(
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
				worktree_path = %orchestrator::relative_worktree_path(project, &issue_run.worktree),
				"Completed issue run."
			);

			Ok(summary)
		},
		Err(error) => {
			state_store.update_run_status(&issue_run.run_id, "failed")?;

			orchestrator::handle_failure(
				tracker,
				project,
				workflow,
				state_store,
				&issue_run,
				&error,
			)?;

			Err(error)
		},
	}
}

pub(crate) fn execute_issue_run_inner<T>(
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
	let privacy_classifier =
		runtime_context::configured_public_projection_privacy_classifier(project)?;
	let review_context = orchestrator::build_review_run_context(project, state_store, issue_run)?;
	let tracker_tool_bridge =
		TrackerToolBridge::with_run_context_state_store_and_privacy_classifier(
			tracker,
			&issue_run.issue,
			workflow,
			review_context.clone(),
			state_store,
			&privacy_classifier,
		);

	if let Some(summary) = closeout::maybe_execute_deterministic_closeout(
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

	context::write_git_credentials_operation_marker(issue_run);

	let agent_git_credentials = credentials::prepare_agent_git_credentials(
		project,
		&issue_run.run_id,
		&issue_run.worktree.path,
	)?;
	let codex_account_pool =
		project.codex().accounts().map(CodexAccountPool::from_config).transpose()?;
	let closeout_review_state_inspector =
		runtime_context::build_closeout_review_state_inspector(project);
	let continuation_guard = app_server::build_issue_turn_continuation_guard(
		tracker,
		&tracker_tool_bridge,
		workflow,
		project,
		issue_run,
		Some(&closeout_review_state_inspector),
	);
	let decodex_tool_bridge = DecodexToolBridge::new(
		&tracker_tool_bridge,
		context::build_decodex_run_context(workflow, issue_run),
	);
	let phase_goal_controller =
		orchestrator::build_phase_goal_controller(project, workflow, state_store, issue_run);
	let run_result = match app_server::execute_issue_app_server_run(IssueAppServerRun {
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
		return Ok(summary::continuation_boundary_summary(
			project,
			workflow,
			issue_run,
			&run_result,
		));
	}

	app_server::finalize_completed_app_server_run(CompletedAppServerRun {
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
