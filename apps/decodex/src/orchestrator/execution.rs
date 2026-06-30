use agent::{
	AppServerThreadArchiveOutcome, AppServerThreadArchiveRequest, CodexAccountAuthFailure,
	CodexAccountPool, CodexAccountProvider,
};
use git_credentials::GitSigningConfig;
use records::LinearExecutionEventPublicProjection;
use sha2::Digest;

use crate::tracker::privacy_classifier::PublicProjectionPrivacyClassifier;

const LOOP_GUARDRAIL_CONVERGENCE_BUDGET: i64 = 3;
const ARCHITECTURE_RECOVERY_BUDGET: usize = 1;
const ARCHITECTURE_RECOVERY_RETRY_KIND: &str = "architecture_recovery";
const REVIEW_HANDOFF_STATE_DRIFT_DETECTED_EVENT_TYPE: &str = "review_handoff_state_drift_detected";
const REVIEW_HANDOFF_STATE_DRIFT_RECOVERED_EVENT_TYPE: &str =
	"review_handoff_state_drift_recovered";
const REVIEW_HANDOFF_REBOUND_ORCHESTRATION_PHASE: &str = "request_pending";
const RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE: &str = "retryable_failed_start_cleanup";

#[derive(Debug)]
pub(crate) struct AppServerZeroEvidenceStartFailure {
	issue_identifier: String,
	run_id: String,
}
impl AppServerZeroEvidenceStartFailure {
	fn new(issue_identifier: String, run_id: String) -> Self {
		Self { issue_identifier, run_id }
	}

	fn error_class(&self) -> &'static str {
		"app_server_zero_evidence_start_failed"
	}

	fn terminal_next_action(&self, recovery_gate: &str) -> String {
		format!(
			"inspect local app-server startup logs and Decodex account/runtime state for run `{}`, verify `decodex probe stdio://`, restart `decodex serve` if needed, {recovery_gate}",
			self.run_id
		)
	}

	fn retry_next_action(&self) -> String {
		format!(
			"restart the app-server and retry automatically for run `{}`; inspect private startup diagnostics if the retry budget exhausts",
			self.run_id
		)
	}
}

impl Display for AppServerZeroEvidenceStartFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(
			formatter,
			"App-server run `{}` for issue `{}` failed before Decodex recorded a thread, turn, protocol event, or private execution event.",
			self.run_id, self.issue_identifier
		)
	}
}

impl Error for AppServerZeroEvidenceStartFailure {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetainedReviewRepairPushFailureKind {
	Auth,
	Refspec,
	RemoteRejected,
	Failed,
}
impl RetainedReviewRepairPushFailureKind {
	fn error_class(self) -> &'static str {
		match self {
			Self::Auth => "retained_review_repair_push_auth_failed",
			Self::Refspec => "retained_review_repair_push_refspec_failed",
			Self::RemoteRejected => "retained_review_repair_push_remote_rejected",
			Self::Failed => "retained_review_repair_push_failed",
		}
	}
}

#[derive(Debug)]
pub(crate) struct RetainedReviewRepairPushFailed {
	issue_identifier: String,
	run_id: String,
	branch_name: String,
	pr_url: Option<String>,
	kind: RetainedReviewRepairPushFailureKind,
	detail: String,
}
impl RetainedReviewRepairPushFailed {
	fn error_class(&self) -> &'static str {
		self.kind.error_class()
	}

	fn terminal_next_action(&self, recovery_gate: &str) -> String {
		match self.kind {
			RetainedReviewRepairPushFailureKind::Auth => format!(
				"repair GitHub authentication, then rerun retained review repair for branch `{}`, {recovery_gate}",
				self.branch_name
			),
			RetainedReviewRepairPushFailureKind::Refspec => format!(
				"inspect retained review-repair branch `{}` and push refspec, repair the branch/ref mismatch manually, {recovery_gate}",
				self.branch_name
			),
			RetainedReviewRepairPushFailureKind::RemoteRejected => format!(
				"inspect remote branch `{}` for non-fast-forward or protection drift, reconcile the retained PR branch, {recovery_gate}",
				self.branch_name
			),
			RetainedReviewRepairPushFailureKind::Failed => format!(
				"inspect the retained review-repair push failure for branch `{}`, repair the remote update blocker, {recovery_gate}",
				self.branch_name
			),
		}
	}
}

impl Display for RetainedReviewRepairPushFailed {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(
			formatter,
			"Run `{}` for issue `{}` could not push retained review-repair branch `{}` before handoff validation: {}",
			self.run_id, self.issue_identifier, self.branch_name, self.detail
		)
	}
}

impl Error for RetainedReviewRepairPushFailed {}

struct AgentGitCredentialEnvironment {
	process_env: AppServerProcessEnv,
}
impl AgentGitCredentialEnvironment {
	fn process_env(&self) -> &AppServerProcessEnv {
		&self.process_env
	}
}

#[derive(Clone, Copy)]
struct TerminalFailureWritebackRuntime<'a> {
	service_id: &'a str,
	state_store: Option<&'a StateStore>,
	privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
}

struct PreparedTerminalFailureWriteback {
	failure_state_id: String,
	needs_attention_label: String,
	needs_attention_label_id: Option<String>,
	terminal_failure_state_name: String,
	projection: LinearExecutionEventPublicProjection,
	error_class: &'static str,
	retry_guarded_by_state: bool,
}

struct ArchitectureRecoveryStart {
	attempt_number: usize,
	max_attempts: usize,
	policy_decision: AuthorityBoundaryPolicyDecision,
	detail: String,
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

struct ZeroEvidenceAppServerStartFailureContext {
	protocol_event_count: i64,
	private_event_count: usize,
	thread_recorded: bool,
	turn_recorded: bool,
}

struct ZeroEvidenceAppServerStartFailureDiagnostic {
	source_error_summary: String,
	source_error_chain: Vec<String>,
}

struct LoopGuardrailWorktreeFingerprint {
	head_sha: String,
	tracked_status_hash: String,
	tracked_diff_hash: String,
	effective_status_hash: String,
	branch_delta_present: bool,
	effective_delta_present: bool,
}

struct FailureHandlingContext<'a, T>
where
	T: IssueTracker,
{
	tracker: &'a T,
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	issue_run: &'a IssueRunPlan,
	worktree_path: &'a str,
	retry_budget_attempts: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RunFailureWritebackDisposition {
	RetryableGeneric,
	RetryableStructuredRecovery,
	TerminalAttention,
}
impl RunFailureWritebackDisposition {
	fn requires_terminal_attention(self) -> bool {
		self == Self::TerminalAttention
	}

	fn preserves_retry_through_zero_evidence(self) -> bool {
		self == Self::RetryableStructuredRecovery
	}
}

enum IssueAppServerRunOutcome {
	Completed(AppServerRunResult),
	Finalized(RunSummary),
}

enum LoopGuardrailRecoveryDecision {
	Start(ArchitectureRecoveryStart),
	HumanRequired(LoopGuardrailStopRequested),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReviewHandoffFailureDriftLineage {
	Exact,
	Descends,
	Diverged,
	Unknown,
}
impl ReviewHandoffFailureDriftLineage {
	fn allows_lifecycle_recovery(self) -> bool {
		matches!(self, Self::Exact | Self::Descends)
	}

	fn as_str(self) -> &'static str {
		match self {
			Self::Exact => "exact",
			Self::Descends => "descends",
			Self::Diverged => "diverged",
			Self::Unknown => "unknown",
		}
	}
}

enum ReviewHandoffStateDriftTransition {
	AlreadySuccess,
	MoveToSuccess(String),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TerminalFailureEventRecordStatus {
	Recorded,
	Duplicate,
	NoLocalStore,
}

pub(crate) fn run_failure_writeback_disposition(error: &Report) -> RunFailureWritebackDisposition {
	if error.downcast_ref::<ManualAttentionRequested>().is_some()
		|| error.downcast_ref::<LoopGuardrailStopRequested>().is_some()
		|| error
			.downcast_ref::<AppServerPhaseGoalFailure>()
			.is_some_and(|failure| !failure.is_terminal_path_missing())
		|| error.downcast_ref::<ReviewHandoffNeedsAttention>().is_some()
		|| error.downcast_ref::<RetainedReviewRepairPushFailed>().is_some()
		|| error.downcast_ref::<RetainedPartialProgress>().is_some()
		|| error
			.downcast_ref::<AppServerCapabilityPreflightFailure>()
			.is_some_and(|failure| !failure.is_retryable_timeout())
		|| error.downcast_ref::<AppServerHomePreflightFailure>().is_some()
		|| error.downcast_ref::<CodexAccountAuthFailure>().is_some()
		|| error
			.downcast_ref::<AppServerTransportFailure>()
			.is_some_and(|failure| !failure.is_retryable_startup())
		|| error.downcast_ref::<AgentGitCredentialsUnavailable>().is_some()
		|| error
			.downcast_ref::<AppServerTurnFailure>()
			.is_some_and(AppServerTurnFailure::requires_operator_attention)
		|| error.downcast_ref::<ReviewPolicyStopRequested>().is_some()
		|| error.downcast_ref::<RepoGateFailure>().is_some_and(|repo_gate_failure| {
			repo_gate_failure.disposition() == RepoGateFailureDisposition::NeedsHumanAttention
		}) {
		RunFailureWritebackDisposition::TerminalAttention
	} else if error.downcast_ref::<AppServerZeroEvidenceStartFailure>().is_some()
		|| error
			.downcast_ref::<AppServerCapabilityPreflightFailure>()
			.is_some_and(AppServerCapabilityPreflightFailure::is_retryable_timeout)
		|| error.downcast_ref::<StalledRunNeedsAttention>().is_some()
		|| error.downcast_ref::<RepoGateFailure>().is_some_and(|repo_gate_failure| {
			matches!(
				repo_gate_failure.disposition(),
				RepoGateFailureDisposition::ContinueRepair
					| RepoGateFailureDisposition::RetryAfterBackoff
			)
		}) || error
		.downcast_ref::<AppServerTransportFailure>()
		.is_some_and(AppServerTransportFailure::is_retryable_startup)
		|| error
			.downcast_ref::<AppServerPhaseGoalFailure>()
			.is_some_and(AppServerPhaseGoalFailure::is_terminal_path_missing)
		|| error.downcast_ref::<AppServerDynamicToolFailure>().is_some()
		|| error.downcast_ref::<AppServerTurnFailure>().is_some()
	{
		RunFailureWritebackDisposition::RetryableStructuredRecovery
	} else {
		RunFailureWritebackDisposition::RetryableGeneric
	}
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
			error_class: None,
		})
		.wrap_err(error);
	}

	error
}

fn preserve_and_promote_app_server_run_failure(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	workflow: &WorkflowDocument,
	completion_disposition: Result<RunCompletionDisposition>,
	error: Report,
) -> Report {
	let error =
		preserve_manual_attention_request(completion_disposition, issue_run, workflow, error);

	promote_zero_evidence_app_server_start_failure(project, state_store, issue_run, error)
}

fn promote_zero_evidence_app_server_start_failure(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: Report,
) -> Report {
	let writeback_disposition = run_failure_writeback_disposition(&error);

	if writeback_disposition.requires_terminal_attention()
		|| writeback_disposition.preserves_retry_through_zero_evidence()
	{
		return error;
	}

	match zero_evidence_app_server_start_failure_context(project, state_store, issue_run) {
		Ok(Some(context)) => {
			let diagnostic = zero_evidence_app_server_start_failure_diagnostic(&error);

			if let Err(record_error) = record_zero_evidence_app_server_start_failure(
				project,
				state_store,
				issue_run,
				&context,
				&diagnostic,
			) {
				tracing::warn!(
					?record_error,
					project_id = project.service_id(),
					issue_id = issue_run.issue.id,
					issue = issue_run.issue.identifier,
					run_id = issue_run.run_id,
					attempt = issue_run.attempt_number,
					"Failed to record zero-evidence app-server start failure evidence."
				);
			}

			Report::new(AppServerZeroEvidenceStartFailure::new(
				issue_run.issue.identifier.clone(),
				issue_run.run_id.clone(),
			))
			.wrap_err(error)
		},
		Ok(None) => error,
		Err(context_error) => {
			tracing::warn!(
				?context_error,
				project_id = project.service_id(),
				issue_id = issue_run.issue.id,
				issue = issue_run.issue.identifier,
				run_id = issue_run.run_id,
				attempt = issue_run.attempt_number,
				"Failed to classify app-server start failure evidence."
			);

			error
		},
	}
}

fn zero_evidence_app_server_start_failure_context(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<Option<ZeroEvidenceAppServerStartFailureContext>> {
	let protocol_event_count = state_store.event_count(&issue_run.run_id)?;
	let private_event_count = state_store
		.list_private_execution_events(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)?
		.len();
	let run_attempt = state_store.run_attempt(&issue_run.run_id)?;
	let thread_recorded = run_attempt.as_ref().and_then(|attempt| attempt.thread_id()).is_some();
	let turn_recorded = run_attempt.as_ref().and_then(|attempt| attempt.turn_id()).is_some();

	if protocol_event_count == 0 && private_event_count == 0 && !thread_recorded && !turn_recorded {
		Ok(Some(ZeroEvidenceAppServerStartFailureContext {
			protocol_event_count,
			private_event_count,
			thread_recorded,
			turn_recorded,
		}))
	} else {
		Ok(None)
	}
}

fn record_zero_evidence_app_server_start_failure(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	context: &ZeroEvidenceAppServerStartFailureContext,
	diagnostic: &ZeroEvidenceAppServerStartFailureDiagnostic,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			"app_server_zero_evidence_start_failure",
			json!({
				"error_class": "app_server_zero_evidence_start_failed",
				"summary": "App-server dispatch failed before Decodex recorded a thread, turn, protocol event, or private execution event.",
				"issue_identifier": issue_run.issue.identifier.as_str(),
				"attempt_number": issue_run.attempt_number,
				"branch": issue_run.worktree.branch_name.as_str(),
				"worktree_path": issue_run.worktree.path.display().to_string(),
				"protocol_event_count": context.protocol_event_count,
				"private_event_count": context.private_event_count,
				"thread_recorded": context.thread_recorded,
				"turn_recorded": context.turn_recorded,
				"source_error_summary": diagnostic.source_error_summary.as_str(),
				"source_error_chain": &diagnostic.source_error_chain,
			}),
		)
		.map(|_| ())
}

fn zero_evidence_app_server_start_failure_diagnostic(
	error: &Report,
) -> ZeroEvidenceAppServerStartFailureDiagnostic {
	let source_error_chain = error
		.chain()
		.map(|cause| sanitize_private_diagnostic_text(&cause.to_string()))
		.collect::<Vec<_>>();
	let source_error_summary = source_error_chain
		.first()
		.cloned()
		.unwrap_or_else(|| String::from("unknown app-server startup failure"));

	ZeroEvidenceAppServerStartFailureDiagnostic { source_error_summary, source_error_chain }
}

fn sanitize_private_diagnostic_text(text: &str) -> String {
	let mut sanitized = text.to_owned();

	for (name, value) in env::vars() {
		if !diagnostic_env_var_name_is_sensitive(&name) || value.len() < 6 {
			continue;
		}

		let replacement = format!("<redacted env:{name}>");

		sanitized = sanitized.replace(&value, &replacement);
	}

	truncate_private_diagnostic_text(&sanitized)
}

fn diagnostic_env_var_name_is_sensitive(name: &str) -> bool {
	let normalized = name.to_ascii_lowercase();

	normalized.contains("token")
		|| normalized.contains("secret")
		|| normalized.contains("password")
		|| normalized.contains("credential")
		|| normalized.contains("api_key")
		|| normalized.contains("apikey")
		|| normalized.ends_with("_pat")
		|| normalized.starts_with("pat_")
		|| normalized.contains("_pat_")
		|| normalized.contains("auth")
}

fn truncate_private_diagnostic_text(text: &str) -> String {
	const MAX_PRIVATE_DIAGNOSTIC_TEXT_CHARS: usize = 2_000;

	if text.chars().count() <= MAX_PRIVATE_DIAGNOSTIC_TEXT_CHARS {
		return text.to_owned();
	}

	let mut truncated = text.chars().take(MAX_PRIVATE_DIAGNOSTIC_TEXT_CHARS).collect::<String>();

	truncated.push_str("...<truncated>");

	truncated
}

fn try_recover_review_handoff_failure_drift<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<bool>
where
	T: IssueTracker,
{
	if !review_handoff_failure_drift_can_handle(error) {
		return Ok(false);
	}

	let Some(worktree_fingerprint) = loop_guardrail_worktree_fingerprint(&issue_run.worktree.path)?
	else {
		return Ok(false);
	};

	if worktree_fingerprint.effective_delta_present {
		return Ok(false);
	}

	let Some(review_handoff) = state_store.review_handoff_marker(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.worktree.branch_name,
	)?
	else {
		return Ok(false);
	};

	if review_handoff.branch_name() != issue_run.worktree.branch_name
		|| review_handoff.pr_head_ref_name() != issue_run.worktree.branch_name
	{
		return Ok(false);
	}

	let lineage = review_handoff_failure_drift_lineage(
		&issue_run.worktree.path,
		review_handoff.pr_head_oid(),
		&worktree_fingerprint.head_sha,
	);

	if !lineage.allows_lifecycle_recovery() {
		return Ok(false);
	}

	let tracker_policy = workflow.frontmatter().tracker();
	let success_state = tracker_policy.success_state();
	let current_state = issue_run.issue.state.name.as_str();
	let Some(success_state_transition) =
		review_handoff_state_drift_success_transition(workflow, issue_run)?
	else {
		return Ok(false);
	};
	let issue_state_recovered =
		matches!(success_state_transition, ReviewHandoffStateDriftTransition::MoveToSuccess(_));
	let rebounded_orchestration = rebound_review_handoff_orchestration_marker(
		project,
		state_store,
		issue_run,
		&review_handoff,
		&worktree_fingerprint.head_sha,
	)?;
	let needs_attention_cleared = tracker::set_issue_label_presence(
		tracker,
		&issue_run.issue,
		tracker_policy.needs_attention_label(),
		false,
	)?;

	if let ReviewHandoffStateDriftTransition::MoveToSuccess(state_id) = success_state_transition {
		tracker.update_issue_state(&issue_run.issue.id, &state_id)?;
	}

	state_store
		.clear_loop_guardrail_checkpoints_for_issue(project.service_id(), &issue_run.issue.id)?;
	state_store.update_run_status(&issue_run.run_id, "succeeded")?;
	state_store
		.append_private_execution_event(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			REVIEW_HANDOFF_STATE_DRIFT_RECOVERED_EVENT_TYPE,
			json!({
				"schema": "decodex.review_handoff_state_drift_recovered/1",
				"reason": "current_review_handoff_marker",
				"source_error_class": review_handoff_failure_drift_source_error_class(error),
				"branch_name": issue_run.worktree.branch_name,
				"pr_url": review_handoff.pr_url(),
				"marker_head_sha": review_handoff.pr_head_oid(),
				"local_head_sha": worktree_fingerprint.head_sha,
				"lineage": lineage.as_str(),
				"previous_issue_state": current_state,
				"target_issue_state": success_state,
				"issue_state_recovered": issue_state_recovered,
				"needs_attention_cleared": needs_attention_cleared,
				"orchestration_rebound": rebounded_orchestration,
			}),
		)
		.map(|_| ())?;

	tracing::warn!(
		project_id = project.service_id(),
		issue_id = issue_run.issue.id,
		issue = issue_run.issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		branch = issue_run.worktree.branch_name,
		pr_url = review_handoff.pr_url(),
		lineage = lineage.as_str(),
		"Recovered review handoff state drift before retry/no-diff failure writeback."
	);

	Ok(true)
}

fn review_handoff_state_drift_success_transition(
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
) -> Result<Option<ReviewHandoffStateDriftTransition>> {
	let tracker_policy = workflow.frontmatter().tracker();
	let success_state = tracker_policy.success_state();
	let current_state = issue_run.issue.state.name.as_str();

	if current_state == success_state {
		return Ok(Some(ReviewHandoffStateDriftTransition::AlreadySuccess));
	}
	if current_state != tracker_policy.in_progress_state()
		&& current_state != tracker_policy.failure_state()
	{
		return Ok(None);
	}

	let state_id = issue_run.issue.state_id_for_name(success_state).ok_or_else(|| {
		eyre::eyre!(
			"State `{success_state}` was not found for issue `{}` during review handoff state drift recovery.",
			issue_run.issue.identifier
		)
	})?;

	Ok(Some(ReviewHandoffStateDriftTransition::MoveToSuccess(state_id.to_owned())))
}

fn rebound_review_handoff_orchestration_marker(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	review_handoff: &ReviewHandoffMarker,
	local_head_sha: &str,
) -> Result<bool> {
	let existing_orchestration = state_store.review_orchestration_marker(
		project.service_id(),
		&issue_run.issue.id,
		review_handoff,
	)?;
	let rebounded_orchestration = existing_orchestration.as_ref().is_none_or(|marker| {
		marker.branch_name() != review_handoff.branch_name()
			|| marker.pr_url() != review_handoff.pr_url()
			|| marker.head_sha() != local_head_sha
			|| marker.phase() != REVIEW_HANDOFF_REBOUND_ORCHESTRATION_PHASE
	});
	let orchestration_marker = ReviewOrchestrationMarker::new(
		review_handoff.run_id().to_owned(),
		review_handoff.attempt_number(),
		review_handoff.branch_name().to_owned(),
		review_handoff.pr_url().to_owned(),
		local_head_sha.to_owned(),
		REVIEW_HANDOFF_REBOUND_ORCHESTRATION_PHASE,
		None,
		None,
		None,
		0,
		existing_orchestration.as_ref().map_or(0, ReviewOrchestrationMarker::external_round_count),
		None,
	);

	state_store.upsert_review_orchestration_marker(
		project.service_id(),
		&issue_run.issue.id,
		&orchestration_marker,
	)?;

	Ok(rebounded_orchestration)
}

fn review_handoff_failure_drift_can_handle(error: &Report) -> bool {
	!run_failure_requires_terminal_attention(error)
		&& error.downcast_ref::<ManualAttentionRequested>().is_none()
		&& error.downcast_ref::<LoopGuardrailStopRequested>().is_none()
		&& error.downcast_ref::<ReviewHandoffNeedsAttention>().is_none()
		&& error.downcast_ref::<RetainedReviewNeedsAttention>().is_none()
		&& error.downcast_ref::<ReviewPolicyStopRequested>().is_none()
		&& error.downcast_ref::<CodexAccountAuthFailure>().is_none()
}

fn review_handoff_failure_drift_source_error_class(error: &Report) -> &'static str {
	retained_progress_source_error_class(error).unwrap_or("retryable_execution_failure")
}

fn review_handoff_failure_drift_lineage(
	worktree_path: &Path,
	recorded_head_oid: &str,
	local_head_oid: &str,
) -> ReviewHandoffFailureDriftLineage {
	if recorded_head_oid == local_head_oid {
		return ReviewHandoffFailureDriftLineage::Exact;
	}

	let Ok(output) = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["merge-base", "--is-ancestor", recorded_head_oid, local_head_oid])
		.output()
	else {
		return ReviewHandoffFailureDriftLineage::Unknown;
	};

	match output.status.code() {
		Some(0) => ReviewHandoffFailureDriftLineage::Descends,
		Some(1) => ReviewHandoffFailureDriftLineage::Diverged,
		_ => ReviewHandoffFailureDriftLineage::Unknown,
	}
}

fn review_handoff_state_drift_attention_error(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<Option<ManualAttentionRequested>> {
	if !review_handoff_failure_drift_can_handle(error) {
		return Ok(None);
	}

	let Some(worktree_fingerprint) = loop_guardrail_worktree_fingerprint(&issue_run.worktree.path)?
	else {
		return Ok(None);
	};

	if worktree_fingerprint.effective_delta_present {
		return Ok(None);
	}

	let checkpoint = state_store.review_policy_checkpoint(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.run_id,
		issue_run.attempt_number,
		"handoff",
	)?;
	let drift_reason = match state_store.review_handoff_marker(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.worktree.branch_name,
	)? {
		Some(review_handoff) => review_handoff_marker_drift_reason(
			workflow,
			issue_run,
			&worktree_fingerprint,
			&review_handoff,
		)?,
		None => {
			let Some(checkpoint) = checkpoint.as_ref() else {
				return Ok(None);
			};

			if checkpoint.status() != "clean"
				|| checkpoint.head_sha() != worktree_fingerprint.head_sha
			{
				return Ok(None);
			}

			Some(String::from("missing_review_handoff_marker"))
		},
	};
	let Some(drift_reason) = drift_reason else {
		return Ok(None);
	};

	state_store
		.append_private_execution_event(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			REVIEW_HANDOFF_STATE_DRIFT_DETECTED_EVENT_TYPE,
			json!({
				"schema": "decodex.review_handoff_state_drift_detected/1",
				"reason": drift_reason,
				"source_error_class": review_handoff_failure_drift_source_error_class(error),
				"branch_name": issue_run.worktree.branch_name,
				"checkpoint_status": checkpoint.as_ref().map(|checkpoint| checkpoint.status()),
				"checkpoint_head_sha": checkpoint.as_ref().map(|checkpoint| checkpoint.head_sha()),
				"local_head_sha": worktree_fingerprint.head_sha,
				"next_action": "restore or rebind the retained review handoff marker before retrying execution",
			}),
		)
		.map(|_| ())?;

	Ok(Some(ManualAttentionRequested {
		issue_identifier: issue_run.issue.identifier.clone(),
		label: workflow.frontmatter().tracker().needs_attention_label().to_owned(),
		run_id: issue_run.run_id.clone(),
		error_class: Some(LoopGuardrailReason::ReviewHandoffStateDrift.error_class().to_owned()),
	}))
}

fn review_handoff_marker_drift_reason(
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	worktree_fingerprint: &LoopGuardrailWorktreeFingerprint,
	review_handoff: &ReviewHandoffMarker,
) -> Result<Option<String>> {
	if review_handoff.branch_name() != issue_run.worktree.branch_name {
		return Ok(Some(String::from("review_handoff_marker_branch_mismatch")));
	}
	if review_handoff.pr_head_ref_name() != issue_run.worktree.branch_name {
		return Ok(Some(String::from("review_handoff_marker_pr_head_ref_mismatch")));
	}

	let lineage = review_handoff_failure_drift_lineage(
		&issue_run.worktree.path,
		review_handoff.pr_head_oid(),
		&worktree_fingerprint.head_sha,
	);

	if !lineage.allows_lifecycle_recovery() {
		return Ok(Some(format!("review_handoff_marker_{}", lineage.as_str())));
	}
	if review_handoff_state_drift_success_transition(workflow, issue_run)?.is_some() {
		return Ok(None);
	}

	Ok(Some(String::from("review_handoff_marker_issue_state_unsupported")))
}

fn retryable_failure_loop_guardrail_stop(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<Option<LoopGuardrailStopRequested>> {
	let Some(worktree_fingerprint) = loop_guardrail_worktree_fingerprint(&issue_run.worktree.path)?
	else {
		return Ok(None);
	};
	let repo_gate_diagnostic = error
		.downcast_ref::<RepoGateFailure>()
		.and_then(RepoGateFailure::diagnostic)
		.map(RepoGateFailureDiagnostic::to_json);
	let mut observations = Vec::new();

	if let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>()
		&& repo_gate_failure.disposition() == RepoGateFailureDisposition::ContinueRepair
	{
		observations.push((
			LoopGuardrailReason::ValidationRepeat,
			format!(
				"{}:{}:{}",
				repo_gate_failure.error_class(),
				worktree_fingerprint.head_sha.as_str(),
				loop_guardrail_text_hash(&error.to_string())
			),
			Some(repo_gate_failure.error_class()),
		));
		observations.push((
			LoopGuardrailReason::RemainingDeltaUnchanged,
			format!(
				"{}:{}:{}:{}",
				repo_gate_failure.error_class(),
				worktree_fingerprint.head_sha.as_str(),
				worktree_fingerprint.effective_status_hash.as_str(),
				worktree_fingerprint.tracked_diff_hash.as_str()
			),
			Some(repo_gate_failure.error_class()),
		));
	}

	if !worktree_fingerprint.effective_delta_present {
		observations.push((
			LoopGuardrailReason::NoEffectiveDiff,
			format!(
				"{}:{}",
				worktree_fingerprint.effective_status_hash.as_str(),
				worktree_fingerprint.tracked_diff_hash.as_str()
			),
			retained_progress_source_error_class(error),
		));
	}

	for (reason, fingerprint, source_error_class) in observations {
		let mut details = json!({
			"schema": "decodex.loop_guardrail_checkpoint/1",
			"reason": reason.error_class(),
			"source_error_class": source_error_class,
			"head_sha": worktree_fingerprint.head_sha.as_str(),
			"tracked_status_hash": worktree_fingerprint.tracked_status_hash.as_str(),
			"tracked_diff_hash": worktree_fingerprint.tracked_diff_hash.as_str(),
			"effective_status_hash": worktree_fingerprint.effective_status_hash.as_str(),
			"branch_delta_present": worktree_fingerprint.branch_delta_present,
			"effective_delta_present": worktree_fingerprint.effective_delta_present,
			"threshold": LOOP_GUARDRAIL_CONVERGENCE_BUDGET,
		});

		if let Some(diagnostic) = &repo_gate_diagnostic {
			details["repo_gate_failure"] = diagnostic.clone();
		}

		let details_json = details.to_string();
		let checkpoint =
			state_store.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
				project_id: project.service_id(),
				issue_id: &issue_run.issue.id,
				reason: reason.error_class(),
				fingerprint: &fingerprint,
				run_id: &issue_run.run_id,
				attempt_number: issue_run.attempt_number,
				details_json: &details_json,
			})?;

		record_loop_guardrail_private_event(
			project,
			state_store,
			issue_run,
			&checkpoint,
			source_error_class,
		)?;

		if checkpoint.consecutive_count() >= LOOP_GUARDRAIL_CONVERGENCE_BUDGET {
			return Ok(Some(LoopGuardrailStopRequested {
				issue_identifier: issue_run.issue.identifier.clone(),
				run_id: issue_run.run_id.clone(),
				reason,
				consecutive_count: checkpoint.consecutive_count(),
				fingerprint,
				source_error_class: source_error_class.map(ToOwned::to_owned),
				architecture_recovery_reason_code: None,
			}));
		}
	}

	Ok(None)
}

fn loop_guardrail_worktree_fingerprint(
	worktree_path: &Path,
) -> Result<Option<LoopGuardrailWorktreeFingerprint>> {
	let Some(head_sha) = worktree_head_oid(worktree_path)? else {
		return Ok(None);
	};
	let Some(tracked_status) =
		git_guardrail_output(worktree_path, &["status", "--porcelain", "--untracked-files=no"])?
	else {
		return Ok(None);
	};
	let Some(raw_status) = git_guardrail_output(worktree_path, &["status", "--porcelain"])? else {
		return Ok(None);
	};
	let Some(tracked_diff) =
		git_guardrail_output(worktree_path, &["diff", "--binary", "--no-ext-diff", "HEAD", "--"])?
	else {
		return Ok(None);
	};
	let effective_status = loop_guardrail_effective_status(&raw_status);
	let branch_delta_present = repo_gate_changed_tracked_files(worktree_path)
		.is_ok_and(|changed_files| !changed_files.is_empty());

	Ok(Some(LoopGuardrailWorktreeFingerprint {
		head_sha,
		tracked_status_hash: loop_guardrail_text_hash(&tracked_status),
		tracked_diff_hash: loop_guardrail_text_hash(&tracked_diff),
		effective_status_hash: loop_guardrail_text_hash(&effective_status),
		branch_delta_present,
		effective_delta_present: branch_delta_present
			|| !effective_status.trim().is_empty()
			|| !tracked_diff.trim().is_empty(),
	}))
}

fn loop_guardrail_effective_status(raw_status: &str) -> String {
	let lines = raw_status
		.lines()
		.map(str::trim_end)
		.filter(|line| !line.is_empty())
		.filter(|line| !state::is_untracked_decodex_runtime_artifact_status_line(line))
		.collect::<Vec<_>>();

	if lines.is_empty() {
		return String::new();
	}

	let mut status = lines.join("\n");

	status.push('\n');

	status
}

fn git_guardrail_output(worktree_path: &Path, args: &[&str]) -> Result<Option<String>> {
	let output = Command::new("git").arg("-C").arg(worktree_path).args(args).output()?;

	if !output.status.success() {
		return Ok(None);
	}

	Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
}

fn loop_guardrail_text_hash(text: &str) -> String {
	let digest = <Sha256 as Digest>::digest(text.as_bytes());
	let mut hash = String::with_capacity(64);

	for byte in digest {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	hash
}

fn record_loop_guardrail_private_event(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	checkpoint: &LoopGuardrailCheckpoint,
	source_error_class: Option<&str>,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			"loop_guardrail_checkpoint",
			json!({
				"schema": "decodex.loop_guardrail_checkpoint/1",
				"reason": checkpoint.reason(),
				"fingerprint": checkpoint.fingerprint(),
				"consecutive_count": checkpoint.consecutive_count(),
				"threshold": LOOP_GUARDRAIL_CONVERGENCE_BUDGET,
				"checkpoint_run_id": checkpoint.run_id(),
				"checkpoint_attempt_number": checkpoint.attempt_number(),
				"source_error_class": source_error_class,
				"details": checkpoint.details_json(),
			}),
		)
		.map(|_| ())
}

fn loop_guardrail_stop_from_review_policy(
	review_policy_stop: &ReviewPolicyStopRequested,
) -> LoopGuardrailStopRequested {
	LoopGuardrailStopRequested {
		issue_identifier: review_policy_stop.issue_identifier.clone(),
		run_id: review_policy_stop.run_id.clone(),
		reason: LoopGuardrailReason::ReviewChurn,
		consecutive_count: review_policy_stop.nonclean_rounds.unwrap_or_default(),
		fingerprint: review_policy_stop.fingerprint.clone().unwrap_or_else(|| {
			format!(
				"{}:{}",
				review_policy_stop.head_sha,
				review_policy_stop.nonclean_rounds.unwrap_or_default()
			)
		}),
		source_error_class: Some(review_policy_stop.reason.error_class().to_owned()),
		architecture_recovery_reason_code: None,
	}
}

fn run_failure_requires_terminal_attention(error: &Report) -> bool {
	run_failure_writeback_disposition(error).requires_terminal_attention()
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
	let retry_budget_attempts = retry_budget_attempts_for_current_failure(state_store, issue_run)?;
	let failure_context = FailureHandlingContext {
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		worktree_path: &worktree_path,
		retry_budget_attempts,
	};

	if handle_review_handoff_failure_drift(
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		error,
		&worktree_path,
	)? {
		return Ok(());
	}

	let loop_guardrail_stop = retryable_failure_loop_guardrail_stop_unless_terminal_attention(
		project,
		state_store,
		issue_run,
		error,
		requires_terminal_attention,
	)?;
	let retained_partial_progress =
		retained_partial_progress_error(error, issue_run, &worktree_path);

	if let Some(review_policy_stop) = error.downcast_ref::<ReviewPolicyStopRequested>()
		&& review_policy_stop.reason == ReviewPolicyStopReason::Exhausted
	{
		return match loop_guardrail_architecture_recovery_decision(
			project,
			state_store,
			issue_run,
			loop_guardrail_stop_from_review_policy(review_policy_stop),
			error,
		)? {
			LoopGuardrailRecoveryDecision::Start(recovery) =>
				apply_architecture_recovery_retry_writeback(
					&failure_context,
					recovery,
					max_attempts,
				),
			LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop) =>
				apply_loop_guardrail_failure_writeback(&failure_context, loop_guardrail_stop),
		};
	}
	if let Some(loop_guardrail_stop) = loop_guardrail_stop {
		return match loop_guardrail_architecture_recovery_decision(
			project,
			state_store,
			issue_run,
			loop_guardrail_stop,
			error,
		)? {
			LoopGuardrailRecoveryDecision::Start(recovery) =>
				apply_architecture_recovery_retry_writeback(
					&failure_context,
					recovery,
					max_attempts,
				),
			LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop) =>
				apply_loop_guardrail_failure_writeback(&failure_context, loop_guardrail_stop),
		};
	}

	if !requires_terminal_attention && retry_budget_attempts < max_attempts {
		return apply_retryable_failure_writeback(&failure_context, error, max_attempts);
	}

	let terminal_error = retained_partial_progress.as_ref().unwrap_or(error);

	apply_terminal_attention_failure_writeback(
		&failure_context,
		manual_attention_requested,
		terminal_error,
	)
}

fn handle_review_handoff_failure_drift<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
	worktree_path: &str,
) -> Result<bool>
where
	T: IssueTracker,
{
	if try_recover_review_handoff_failure_drift(
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		error,
	)? {
		return Ok(true);
	}

	let Some(attention_error) = review_handoff_state_drift_attention_error(
		project,
		workflow,
		state_store,
		issue_run,
		error,
	)?
	else {
		return Ok(false);
	};

	apply_review_handoff_state_drift_attention_writeback(
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		worktree_path,
		attention_error,
	)?;

	Ok(true)
}

fn apply_terminal_attention_failure_writeback<T>(
	context: &FailureHandlingContext<'_, T>,
	manual_attention_requested: bool,
	terminal_error: &Report,
) -> Result<()>
where
	T: IssueTracker,
{
	let privacy_classifier = configured_public_projection_privacy_classifier(context.project)?;
	let outcome = apply_terminal_failure_writeback(
		context.tracker,
		TerminalFailureWritebackRuntime {
			service_id: context.project.service_id(),
			state_store: Some(context.state_store),
			privacy_classifier: &privacy_classifier,
		},
		context.workflow,
		context.issue_run,
		context.worktree_path,
		manual_attention_requested,
		terminal_error,
	)?;

	if outcome.retry_guarded_by_state {
		write_terminal_guard_marker(
			&context.issue_run.worktree.path,
			&context.issue_run.run_id,
			context.issue_run.attempt_number,
		)?;

		context
			.state_store
			.update_run_status(&context.issue_run.run_id, TERMINAL_GUARDED_RUN_STATUS)?;
	}

	write_retry_budget_marker(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		context.retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = %context.worktree_path,
		error_class = outcome.error_class,
		"Run failed and now requires operator attention."
	);

	Ok(())
}

fn retryable_failure_loop_guardrail_stop_unless_terminal_attention(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
	requires_terminal_attention: bool,
) -> Result<Option<LoopGuardrailStopRequested>> {
	if requires_terminal_attention {
		Ok(None)
	} else {
		retryable_failure_loop_guardrail_stop(project, state_store, issue_run, error)
	}
}

fn apply_review_handoff_state_drift_attention_writeback<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	worktree_path: &str,
	attention_error: ManualAttentionRequested,
) -> Result<()>
where
	T: IssueTracker,
{
	let terminal_error = Report::new(attention_error);
	let privacy_classifier = configured_public_projection_privacy_classifier(project)?;
	let outcome = apply_terminal_failure_writeback(
		tracker,
		TerminalFailureWritebackRuntime {
			service_id: project.service_id(),
			state_store: Some(state_store),
			privacy_classifier: &privacy_classifier,
		},
		workflow,
		issue_run,
		worktree_path,
		true,
		&terminal_error,
	)?;

	if outcome.retry_guarded_by_state {
		write_terminal_guard_marker(
			&issue_run.worktree.path,
			&issue_run.run_id,
			issue_run.attempt_number,
		)?;

		state_store.update_run_status(&issue_run.run_id, TERMINAL_GUARDED_RUN_STATUS)?;
	}

	Ok(())
}

fn apply_retryable_failure_writeback<T>(
	context: &FailureHandlingContext<'_, T>,
	error: &Report,
	max_attempts: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	let (retry_error_class, retry_next_action) = retry_comment_details(error);

	write_retry_schedule_marker_for_runtime_retry(
		error,
		context.workflow,
		context.issue_run,
		context.retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		retry_budget_attempt = context.retry_budget_attempts,
		max_attempts,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = %context.worktree_path,
		error_class = retry_error_class,
		"Run failed and remains retryable."
	);

	tracker::create_public_comment(
		context.tracker,
		&context.issue_run.issue.id,
		&format_retry_comment(RetryComment {
			run_id: &context.issue_run.run_id,
			attempt_number: context.issue_run.attempt_number,
			retry_budget_attempt_number: context.retry_budget_attempts,
			max_attempts,
			worktree_path: context.worktree_path.to_owned(),
			branch_name: &context.issue_run.worktree.branch_name,
			error_class: retry_error_class,
			next_action: &retry_next_action,
		}),
	)?;

	write_retry_budget_marker(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		context.retry_budget_attempts,
	)?;
	record_harness_outcome_best_effort(
		context.state_store,
		context.project.service_id(),
		context.issue_run,
		HarnessOutcomeKind::RetryableFailure,
		Some(retry_error_class),
		retryable_failure_validation_result(error, retry_error_class),
		None,
	);
	cleanup_retryable_failed_start_ownership(context, error)?;

	Ok(())
}

fn cleanup_retryable_failed_start_ownership<T>(
	context: &FailureHandlingContext<'_, T>,
	error: &Report,
) -> Result<()>
where
	T: IssueTracker,
{
	if !retryable_failed_start_cleanup_allowed(context, error)? {
		return Ok(());
	}

	let tracker_policy = context.workflow.frontmatter().tracker();
	let failure_state_name = tracker_policy.failure_state();
	let failure_state_is_startable =
		tracker_policy.startable_states().iter().any(|state| state == failure_state_name);

	if !failure_state_is_startable {
		tracing::warn!(
			issue_id = context.issue_run.issue.id,
			issue = context.issue_run.issue.identifier,
			target_state = failure_state_name,
			"Retryable failed-start cleanup skipped because the configured failure state is not startable."
		);

		return Ok(());
	}

	let Some(state_id) = context.issue_run.issue.state_id_for_name(failure_state_name) else {
		tracing::warn!(
			issue_id = context.issue_run.issue.id,
			issue = context.issue_run.issue.identifier,
			target_state = failure_state_name,
			"Retryable failed-start cleanup skipped because the target state id was not available."
		);

		return Ok(());
	};

	context.tracker.update_issue_state(&context.issue_run.issue.id, state_id)?;

	ensure_automation_activity_label(
		context.tracker,
		&context.issue_run.issue,
		context.project.service_id(),
		false,
	)?;

	context.state_store.clear_worktree(&context.issue_run.issue.id)?;
	context
		.state_store
		.append_private_execution_event(
			context.project.service_id(),
			&context.issue_run.issue.id,
			&context.issue_run.run_id,
			context.issue_run.attempt_number,
			RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE,
			json!({
				"schema": "decodex.retryable_failed_start_cleanup/1",
				"source_error_class": retained_progress_source_error_class(error)
					.unwrap_or("retryable_execution_failure"),
				"dispatch_mode": context.issue_run.dispatch_mode.as_str(),
				"active_label_cleared": true,
				"worktree_mapping_cleared": true,
				"target_issue_state": failure_state_name,
				"issue_state_reset": true,
				"retryable_by_next_program_pass": true,
			}),
		)
		.map(|_| ())?;

	tracing::info!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = %context.worktree_path,
		issue_state_reset = true,
		"Cleared retryable failed-start ownership after a no-diff Program run failure."
	);

	Ok(())
}

fn retryable_failed_start_cleanup_allowed<T>(
	context: &FailureHandlingContext<'_, T>,
	error: &Report,
) -> Result<bool>
where
	T: IssueTracker,
{
	if context.issue_run.dispatch_mode != IssueDispatchMode::Program {
		return Ok(false);
	}
	if !retryable_failure_happened_before_effective_agent_execution(error) {
		return Ok(false);
	}
	if context.state_store.lease_for_issue(&context.issue_run.issue.id)?.is_some() {
		return Ok(false);
	}
	if context.state_store.issue_has_review_lifecycle_record(
		context.project.service_id(),
		&context.issue_run.issue.id,
	)? {
		return Ok(false);
	}
	if latest_open_issue_phase_goal_before_attempt(
		context.project,
		context.state_store,
		&context.issue_run.issue.id,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
	)?
	.is_some()
	{
		return Ok(false);
	}

	Ok(loop_guardrail_worktree_fingerprint(&context.issue_run.worktree.path)?
		.is_some_and(|fingerprint| !fingerprint.effective_delta_present))
}

fn retryable_failure_happened_before_effective_agent_execution(error: &Report) -> bool {
	error.downcast_ref::<AppServerZeroEvidenceStartFailure>().is_some()
		|| error
			.downcast_ref::<AppServerCapabilityPreflightFailure>()
			.is_some_and(AppServerCapabilityPreflightFailure::is_retryable_timeout)
		|| error
			.downcast_ref::<AppServerTransportFailure>()
			.is_some_and(AppServerTransportFailure::is_retryable_startup)
}

fn retryable_failure_validation_result(
	error: &Report,
	retry_error_class: &str,
) -> Option<&'static str> {
	if retry_error_class.starts_with("repo_gate_")
		|| error.downcast_ref::<RepoGateFailure>().is_some()
	{
		Some("failed")
	} else {
		None
	}
}

fn apply_architecture_recovery_retry_writeback<T>(
	context: &FailureHandlingContext<'_, T>,
	recovery: ArchitectureRecoveryStart,
	max_attempts: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	let retry_attempt = u32::try_from(context.retry_budget_attempts).unwrap_or(u32::MAX).max(1);
	let delay = retry_delay(RetryKind::Failure, retry_attempt, context.workflow);
	let retry_ready_at_unix_epoch = OffsetDateTime::now_utc().unix_timestamp().saturating_add(
		i64::try_from((delay.as_millis().saturating_add(999)) / 1_000).unwrap_or(i64::MAX),
	);
	let recovery_max_attempts =
		max_attempts.saturating_add(i64::try_from(recovery.max_attempts).unwrap_or(0));

	state::write_run_retry_schedule(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		ARCHITECTURE_RECOVERY_RETRY_KIND,
		retry_ready_at_unix_epoch,
	)?;

	write_retry_budget_marker(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		context.retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		recovery_attempt = recovery.attempt_number,
		max_recovery_attempts = recovery.max_attempts,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = %context.worktree_path,
		"Loop guardrail started autonomous architecture recovery."
	);

	tracker::create_public_comment(
		context.tracker,
		&context.issue_run.issue.id,
		&format_retry_comment(RetryComment {
			run_id: &context.issue_run.run_id,
			attempt_number: context.issue_run.attempt_number,
			retry_budget_attempt_number: context.retry_budget_attempts,
			max_attempts: recovery_max_attempts,
			worktree_path: context.worktree_path.to_owned(),
			branch_name: &context.issue_run.worktree.branch_name,
			error_class: "architecture_recovery_started",
			next_action: architecture_recovery_retry_next_action(recovery.policy_decision),
		}),
	)?;

	record_harness_outcome_best_effort(
		context.state_store,
		context.project.service_id(),
		context.issue_run,
		HarnessOutcomeKind::RetryableFailure,
		Some("architecture_recovery_started"),
		Some("architecture_recovery_started"),
		None,
	);

	Ok(())
}

fn apply_loop_guardrail_failure_writeback<T>(
	context: &FailureHandlingContext<'_, T>,
	loop_guardrail_stop: LoopGuardrailStopRequested,
) -> Result<()>
where
	T: IssueTracker,
{
	let terminal_error = Report::new(loop_guardrail_stop);
	let privacy_classifier = configured_public_projection_privacy_classifier(context.project)?;
	let outcome = apply_terminal_failure_writeback(
		context.tracker,
		TerminalFailureWritebackRuntime {
			service_id: context.project.service_id(),
			state_store: Some(context.state_store),
			privacy_classifier: &privacy_classifier,
		},
		context.workflow,
		context.issue_run,
		context.worktree_path,
		false,
		&terminal_error,
	)?;

	if outcome.retry_guarded_by_state {
		write_terminal_guard_marker(
			&context.issue_run.worktree.path,
			&context.issue_run.run_id,
			context.issue_run.attempt_number,
		)?;

		context
			.state_store
			.update_run_status(&context.issue_run.run_id, TERMINAL_GUARDED_RUN_STATUS)?;
	}

	write_retry_budget_marker(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		context.retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = context.worktree_path,
		error_class = outcome.error_class,
		"Run stopped by loop guardrail."
	);

	Ok(())
}

fn retry_budget_attempts_for_current_failure(
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<i64> {
	let state_attempts = state_store.retry_budget_attempt_count(&issue_run.issue.id)?;
	let current_attempt_counts =
		state_store.run_attempt(&issue_run.run_id)?.is_some_and(|attempt| {
			attempt.issue_id() == issue_run.issue.id
				&& matches!(attempt.status(), "failed" | "interrupted" | "terminal_guarded")
		});
	let previous_state_attempts = state_attempts.saturating_sub(i64::from(current_attempt_counts));

	Ok(issue_run.retry_budget_base.max(previous_state_attempts) + i64::from(current_attempt_counts))
}

fn retained_partial_progress_error(
	error: &Report,
	issue_run: &IssueRunPlan,
	worktree_path: &str,
) -> Option<Report> {
	if retained_progress_should_defer_to_terminal_intent(error)
		|| !worktree_has_tracked_changes(&issue_run.worktree.path)
	{
		return None;
	}

	Some(Report::new(RetainedPartialProgress {
		issue_identifier: issue_run.issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		worktree_path: worktree_path.to_owned(),
		source_error_class: retained_progress_source_error_class(error).map(ToOwned::to_owned),
	}))
}

fn retained_progress_should_defer_to_terminal_intent(error: &Report) -> bool {
	error.downcast_ref::<ManualAttentionRequested>().is_some()
		|| error.downcast_ref::<LoopGuardrailStopRequested>().is_some()
		|| error.downcast_ref::<ReviewHandoffNeedsAttention>().is_some()
		|| error.downcast_ref::<RetainedReviewRepairPushFailed>().is_some()
		|| error.downcast_ref::<RetainedPartialProgress>().is_some()
		|| error.downcast_ref::<RetainedReviewNeedsAttention>().is_some()
		|| error.downcast_ref::<ReviewPolicyStopRequested>().is_some()
		|| error.downcast_ref::<CodexAccountAuthFailure>().is_some()
}

fn retained_progress_source_error_class(error: &Report) -> Option<&'static str> {
	if let Some(app_server_failure) = error.downcast_ref::<AppServerZeroEvidenceStartFailure>() {
		Some(app_server_failure.error_class())
	} else if error.downcast_ref::<StalledRunNeedsAttention>().is_some() {
		Some("stalled_run_detected")
	} else if error.downcast_ref::<AgentGitCredentialsUnavailable>().is_some() {
		Some("github_credentials_unavailable")
	} else if let Some(push_failure) = error.downcast_ref::<RetainedReviewRepairPushFailed>() {
		Some(push_failure.error_class())
	} else if let Some(app_server_failure) =
		error.downcast_ref::<AppServerCapabilityPreflightFailure>()
	{
		Some(app_server_failure.error_class())
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerHomePreflightFailure>() {
		Some(app_server_failure.error_class())
	} else if let Some(account_failure) = error.downcast_ref::<CodexAccountAuthFailure>() {
		Some(account_failure.error_class())
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerTransportFailure>() {
		Some(app_server_failure.error_class())
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerPhaseGoalFailure>() {
		Some(app_server_failure.error_class())
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerDynamicToolFailure>() {
		Some(app_server_failure.error_class())
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerTurnFailure>() {
		Some(app_server_failure.error_class())
	} else if let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() {
		Some(repo_gate_failure.error_class())
	} else if let Some(acceptance_failure) = error.downcast_ref::<PhaseAcceptanceCheckFailure>() {
		Some(acceptance_failure.error_class())
	} else {
		None
	}
}

fn write_retry_schedule_marker_for_runtime_retry(
	error: &Report,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	retry_budget_attempts: i64,
) -> Result<()> {
	if error.downcast_ref::<StalledRunNeedsAttention>().is_some() {
		return write_failure_retry_schedule_marker(workflow, issue_run, retry_budget_attempts);
	}
	if error
		.downcast_ref::<AppServerCapabilityPreflightFailure>()
		.is_some_and(AppServerCapabilityPreflightFailure::is_retryable_timeout)
	{
		return write_failure_retry_schedule_marker(workflow, issue_run, retry_budget_attempts);
	}
	if error
		.downcast_ref::<AppServerPhaseGoalFailure>()
		.is_some_and(AppServerPhaseGoalFailure::is_terminal_path_missing)
	{
		return write_failure_retry_schedule_marker(workflow, issue_run, retry_budget_attempts);
	}

	let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() else {
		return Ok(());
	};
	let Some(retry_kind) = repo_gate_failure.retry_schedule_kind() else {
		return Ok(());
	};

	write_retry_schedule_marker(workflow, issue_run, retry_budget_attempts, retry_kind)
}

fn write_failure_retry_schedule_marker(
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	retry_budget_attempts: i64,
) -> Result<()> {
	write_retry_schedule_marker(workflow, issue_run, retry_budget_attempts, "failure")
}

fn write_retry_schedule_marker(
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	retry_budget_attempts: i64,
	retry_kind: &str,
) -> Result<()> {
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
	let writeback = prepare_terminal_failure_writeback(
		tracker,
		runtime,
		workflow,
		issue_run,
		worktree_path,
		manual_attention_requested,
		error,
	)?;
	let event_status =
		record_terminal_failure_writeback_event(tracker, runtime, issue_run, &writeback)?;

	if event_status == TerminalFailureEventRecordStatus::Duplicate {
		return Ok(terminal_failure_outcome(&writeback));
	}

	let writeback_result =
		apply_terminal_failure_tracker_writeback(tracker, runtime, issue_run, &writeback);

	if let Err(error) = writeback_result {
		forget_terminal_failure_writeback_event(runtime, event_status, &writeback)?;

		return Err(error);
	}
	if let Some(state_store) = runtime.state_store {
		let outcome = if writeback.projection.record.event_type == "needs_attention" {
			HarnessOutcomeKind::ManualAttention
		} else {
			HarnessOutcomeKind::TerminalFailure
		};

		record_harness_outcome_best_effort(
			state_store,
			runtime.service_id,
			issue_run,
			outcome,
			Some(writeback.error_class),
			None,
			writeback.projection.record.pr_url.as_deref(),
		);
	}

	Ok(terminal_failure_outcome(&writeback))
}

fn prepare_terminal_failure_writeback<T>(
	tracker: &T,
	runtime: TerminalFailureWritebackRuntime<'_>,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	worktree_path: &str,
	manual_attention_requested: bool,
	error: &Report,
) -> Result<PreparedTerminalFailureWriteback>
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
	let retry_guarded_by_state = needs_attention_label_id.is_none() && failure_state_is_startable;
	let terminal_failure_state_name = if retry_guarded_by_state {
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
	let recovery_gate = terminal_failure_recovery_gate(
		needs_attention_label,
		needs_attention_label_id.is_some(),
		retry_guarded_by_state,
		tracker_policy.in_progress_state(),
	);
	let (error_class, next_action) =
		terminal_failure_comment_details(manual_attention_requested, error, &recovery_gate);
	let pr_url = terminal_failure_pr_url(error);
	let retained_source_error_class = error
		.downcast_ref::<RetainedPartialProgress>()
		.and_then(|partial_progress| partial_progress.source_error_class.as_deref());
	let comment = format_terminal_failure_comment(
		&issue_run.run_id,
		issue_run.attempt_number,
		worktree_path.to_owned(),
		&issue_run.worktree.branch_name,
		pr_url,
		error_class,
		&next_action,
	);
	let event = terminal_failure_lifecycle_event(
		runtime.service_id,
		issue_run,
		TerminalFailureLifecycle {
			error_class,
			next_action: &next_action,
			pr_url,
			target_state: terminal_failure_state_name,
			worktree_path,
			manual_attention_requested,
			retained_source_error_class,
		},
	);
	let projection = tracker::prepare_linear_execution_event_comment(
		&comment,
		&event,
		runtime.privacy_classifier,
	)?;

	Ok(PreparedTerminalFailureWriteback {
		failure_state_id: failure_state_id.to_owned(),
		needs_attention_label: needs_attention_label.to_owned(),
		needs_attention_label_id,
		terminal_failure_state_name: terminal_failure_state_name.to_owned(),
		projection,
		error_class,
		retry_guarded_by_state,
	})
}

fn record_terminal_failure_writeback_event<T>(
	tracker: &T,
	runtime: TerminalFailureWritebackRuntime<'_>,
	issue_run: &IssueRunPlan,
	writeback: &PreparedTerminalFailureWriteback,
) -> Result<TerminalFailureEventRecordStatus>
where
	T: IssueTracker,
{
	let event_status = if let Some(state_store) = runtime.state_store {
		if !state_store.record_linear_execution_event(&writeback.projection.record)? {
			return Ok(TerminalFailureEventRecordStatus::Duplicate);
		}

		TerminalFailureEventRecordStatus::Recorded
	} else {
		TerminalFailureEventRecordStatus::NoLocalStore
	};

	if remote_terminal_failure_writeback_exists(
		tracker,
		runtime,
		issue_run,
		writeback,
		event_status,
	)? {
		return Ok(TerminalFailureEventRecordStatus::Duplicate);
	}

	Ok(event_status)
}

fn remote_terminal_failure_writeback_exists<T>(
	tracker: &T,
	runtime: TerminalFailureWritebackRuntime<'_>,
	issue_run: &IssueRunPlan,
	writeback: &PreparedTerminalFailureWriteback,
	event_status: TerminalFailureEventRecordStatus,
) -> Result<bool>
where
	T: IssueTracker,
{
	let comments = match tracker.list_comments(&issue_run.issue.id) {
		Ok(comments) => comments,
		Err(error) => {
			forget_terminal_failure_writeback_event(runtime, event_status, writeback)?;

			return Err(error);
		},
	};

	if !records::has_linear_execution_event_record(
		&comments,
		&writeback.projection.record.service_id,
		&writeback.projection.record.issue_id,
		&writeback.projection.record.idempotency_key,
	) {
		return Ok(false);
	}

	tracing::debug!(
		service_id = writeback.projection.record.service_id,
		issue_id = issue_run.issue.id,
		issue = issue_run.issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		event_type = writeback.projection.record.event_type,
		"Skipping terminal failure writeback already present in remote Linear ledger."
	);

	Ok(true)
}

fn apply_terminal_failure_tracker_writeback<T>(
	tracker: &T,
	runtime: TerminalFailureWritebackRuntime<'_>,
	issue_run: &IssueRunPlan,
	writeback: &PreparedTerminalFailureWriteback,
) -> Result<()>
where
	T: IssueTracker,
{
	tracker.update_issue_state(&issue_run.issue.id, &writeback.failure_state_id)?;

	apply_needs_attention_label(
		tracker,
		issue_run,
		runtime.service_id,
		&writeback.needs_attention_label,
		writeback.needs_attention_label_id.clone(),
		&writeback.terminal_failure_state_name,
	)?;

	if runtime.state_store.is_some() {
		tracker::create_prepared_linear_execution_event_comment_without_remote_scan(
			tracker,
			&issue_run.issue.id,
			&writeback.projection,
		)?;
	} else {
		tracker::create_prepared_linear_execution_event_comment(
			tracker,
			&issue_run.issue.id,
			&writeback.projection,
		)?;
	}

	Ok(())
}

fn forget_terminal_failure_writeback_event(
	runtime: TerminalFailureWritebackRuntime<'_>,
	event_status: TerminalFailureEventRecordStatus,
	writeback: &PreparedTerminalFailureWriteback,
) -> Result<()> {
	if event_status == TerminalFailureEventRecordStatus::Recorded
		&& let Some(state_store) = runtime.state_store
	{
		state_store.forget_linear_execution_event(&writeback.projection.record.idempotency_key)?;
	}

	Ok(())
}

fn terminal_failure_outcome(
	writeback: &PreparedTerminalFailureWriteback,
) -> TerminalFailureOutcome {
	TerminalFailureOutcome {
		error_class: writeback.error_class,
		retry_guarded_by_state: writeback.retry_guarded_by_state,
	}
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
