mod app_server;
mod closeout;
mod completion;
mod context;
mod credentials;
mod summary;
#[cfg(test)]
pub(crate) use self::completion::{push_retained_review_repair_head, run_completion_repo_gate};
pub(crate) use self::summary::{planned_issue_state_for_dispatch, run_summary_from_issue_run};
pub(crate) use crate::agent::{
	ReviewHandoffContext, ReviewHandoffWritebackFailed, RunCompletionDisposition,
};
pub(crate) use context::write_run_operation_marker_best_effort;
#[cfg(test)] pub(crate) use credentials::AgentGitCredentialEnvironment;
pub(crate) use credentials::prepare_agent_git_credentials;

use self::app_server::{CompletedAppServerRun, IssueAppServerRun, IssueAppServerRunOutcome};
use crate::{
	agent::{CodexAccountPool, CodexAccountProvider},
	orchestrator::{
		self, AgentGitCredentialsUnavailable, AppServerRunResult, CONTINUATION_PENDING_RUN_STATUS,
		Command, DecodexRunContext, DecodexToolBridge, GhPullRequestReviewStateInspector,
		GitCredentialSource, HarnessOutcomeKind, IssueDispatchMode, IssueRunPlan, IssueTracker,
		LaneDecisionSnapshot, ManualAttentionRequested, Path, PhaseGoalKind,
		RUN_OPERATION_GIT_CREDENTIALS, RUN_OPERATION_REPO_GATE, RUN_OPERATION_REVIEW_WRITEBACK,
		RepoGateFailure, RepoGateTrackedRewriteDecision, Report, Result,
		RetainedReviewRepairPushFailed, RetainedReviewRepairPushFailureKind,
		ReviewHandoffNeedsAttention, RunSummary, ServiceConfig, StateStore, TrackerIssue,
		TrackerToolBridge, WorkflowDocument, build_developer_instructions, build_user_input,
		cleanup_completed_post_review_lane, decide_lane_next_action,
		execute_deterministic_closeout, record_harness_outcome_best_effort, repo_gate_output_text,
		resolve_configured_env_var, run_repo_gate_commands, select_repo_gate_for_worktree,
		validate_review_handoff_runtime, validate_review_repair_runtime, worktree_head_oid,
		write_cleanup_complete_lifecycle_event,
	},
	state, tracker,
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
			persist_issue_run_outcome(state_store, &issue_run.run_id, &summary)?;

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

pub(crate) fn persist_issue_run_outcome(
	state_store: &StateStore,
	run_id: &str,
	summary: &RunSummary,
) -> Result<()> {
	state_store.update_run_status(
		run_id,
		if summary.continuation_pending { CONTINUATION_PENDING_RUN_STATUS } else { "succeeded" },
	)
}

pub(crate) fn resolve_resume_thread_id(
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

pub(crate) fn configured_public_projection_privacy_classifier(
	project: &ServiceConfig,
) -> Result<tracker::privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier> {
	tracker::privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier::from_config(
		project.privacy_classifier(),
	)
}

pub(crate) fn build_closeout_review_state_inspector(
	project: &ServiceConfig,
) -> GhPullRequestReviewStateInspector {
	GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
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
	let privacy_classifier = configured_public_projection_privacy_classifier(project)?;
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

	if let Some(summary) = self::closeout::maybe_execute_deterministic_closeout(
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

	self::context::write_git_credentials_operation_marker(issue_run);

	let agent_git_credentials =
		prepare_agent_git_credentials(project, &issue_run.run_id, &issue_run.worktree.path)?;
	let codex_account_pool =
		project.codex().accounts().map(CodexAccountPool::from_config).transpose()?;
	let closeout_review_state_inspector = build_closeout_review_state_inspector(project);
	let continuation_guard = self::app_server::build_issue_turn_continuation_guard(
		tracker,
		&tracker_tool_bridge,
		workflow,
		project,
		issue_run,
		Some(&closeout_review_state_inspector),
	);
	let decodex_tool_bridge = DecodexToolBridge::new(
		&tracker_tool_bridge,
		self::context::build_decodex_run_context(workflow, issue_run),
	);
	let phase_goal_controller =
		orchestrator::build_phase_goal_controller(project, workflow, state_store, issue_run);
	let run_result = match self::app_server::execute_issue_app_server_run(IssueAppServerRun {
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
		return Ok(self::summary::continuation_boundary_summary(
			project,
			workflow,
			issue_run,
			&run_result,
		));
	}

	self::app_server::finalize_completed_app_server_run(CompletedAppServerRun {
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
