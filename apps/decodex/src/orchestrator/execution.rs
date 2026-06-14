use git_credentials::GitSigningConfig;
use agent::CodexAccountAuthFailure;
use agent::CodexAccountPool;
use agent::CodexAccountProvider;
use agent::{AppServerThreadArchiveOutcome, AppServerThreadArchiveRequest};
use records::LinearExecutionEventPublicProjection;
use sha2::Digest;
use state::DecisionContractRecord;

use crate::tracker::privacy_classifier::PublicProjectionPrivacyClassifier;

const LOOP_GUARDRAIL_CONVERGENCE_BUDGET: i64 = 3;
const ARCHITECTURE_RECOVERY_BUDGET: usize = 1;
const ARCHITECTURE_RECOVERY_RETRY_KIND: &str = "architecture_recovery";

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
	pr_url: Option<&'a str>,
	target_state: &'a str,
	worktree_path: &'a str,
	manual_attention_requested: bool,
	retained_source_error_class: Option<&'a str>,
}

struct RunStartedLifecycleFields<'a> {
	worktree_path: &'a str,
	commit_sha: &'a str,
	privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
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
	detail: String,
}

struct PhaseGoalRecoveryContinuation {
	source_phase: PhaseGoalKind,
	next_phase: PhaseGoalKind,
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

struct RepoGatePhaseGoalController<'a> {
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	issue_run: &'a IssueRunPlan,
}
impl RepoGatePhaseGoalController<'_> {
	fn initial_phase_goal_kind(&self) -> PhaseGoalKind {
		match self.issue_run.dispatch_mode {
			IssueDispatchMode::Normal | IssueDispatchMode::Retry =>
				PhaseGoalKind::ImplementToValidationReady,
			IssueDispatchMode::ReviewRepair => PhaseGoalKind::RepairAcceptedReviewFindings,
			IssueDispatchMode::Closeout => PhaseGoalKind::HandoffEvidence,
		}
	}

	fn latest_persisted_phase_goal(&self) -> Result<Option<PhaseGoalKind>> {
		let events = self.state_store.list_private_execution_events(
			self.project.service_id(),
			&self.issue_run.issue.id,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
		)?;

		Ok(events
			.iter()
			.rev()
			.filter(|event| event.event_type() == "phase_goal_next")
			.find_map(|event| event.payload().get("phase").and_then(Value::as_str))
			.and_then(phase_goal_kind_from_str))
	}

	fn validate_phase_goal_output(&self, phase: PhaseGoalKind) -> Result<PhaseGoalTransition> {
		let selected_repo_gate = select_repo_gate_for_worktree(
			self.workflow.frontmatter().execution(),
			&self.issue_run.worktree.path,
		);

		write_run_operation_marker_best_effort(
			&self.issue_run.worktree.path,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
			RUN_OPERATION_REPO_GATE,
		);

		match run_repo_gate_commands(
			selected_repo_gate.canonicalize_commands(),
			selected_repo_gate.verify_commands(),
			&self.issue_run.worktree.path,
		) {
			Ok(()) => {
				self.state_store.clear_loop_guardrail_checkpoints_for_issue(
					self.project.service_id(),
					&self.issue_run.issue.id,
				)?;
				self.record_phase_goal_transition(
					phase,
					"validation_pass",
					json!({ "nextPhase": PhaseGoalKind::HandoffEvidence.as_str() }),
				)?;

				let next_goal = self.phase_goal_spec(PhaseGoalKind::HandoffEvidence, None);

				self.persist_next_phase_goal(&next_goal, "validation_pass")?;

				Ok(PhaseGoalTransition::Continue(next_goal))
			},
			Err(error) => {
				if let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() {
					self.record_phase_goal_transition(
						phase,
						"validation_fail",
						json!({
							"errorClass": repo_gate_failure.error_class(),
							"disposition": repo_gate_failure.disposition().as_str(),
						}),
					)?;

					if repo_gate_failure.disposition() == RepoGateFailureDisposition::ContinueRepair {
						if let Some(loop_guardrail_stop) = retryable_failure_loop_guardrail_stop(
							self.project,
							self.state_store,
							self.issue_run,
							&error,
						)? {
							match loop_guardrail_architecture_recovery_decision(
								self.project,
								self.state_store,
								self.issue_run,
								loop_guardrail_stop,
								&error,
							)? {
								LoopGuardrailRecoveryDecision::Start(recovery) => {
									let next_goal = self.phase_goal_spec(
										PhaseGoalKind::RepairValidationFailures,
										Some(&recovery.detail),
									);

									self.persist_next_phase_goal(&next_goal, "architecture_recovery_started")?;

									return Ok(PhaseGoalTransition::Continue(next_goal));
								},
								LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop) => {
									return Err(Report::new(loop_guardrail_stop).wrap_err(error));
								},
							}
						}

						let detail = format!(
							"Repo gate failed with `{}`. Inspect the worktree, run the registered canonicalize and verify commands, and repair only the validation failure.",
							repo_gate_failure.error_class()
						);
						let next_goal =
							self.phase_goal_spec(PhaseGoalKind::RepairValidationFailures, Some(&detail));

						self.persist_next_phase_goal(&next_goal, "validation_fail")?;

						return Ok(PhaseGoalTransition::Continue(next_goal));
					}
				}

				Err(error)
			},
		}
	}

	fn phase_goal_spec(
		&self,
		phase: PhaseGoalKind,
		detail: Option<&str>,
	) -> PhaseGoalSpec {
		let phase_exit_contract = "Phase exit contract: when this phase objective is satisfied, explicitly mark the active phase goal complete with the Codex goal completion mechanism so Decodex can run its repo gate and select the next phase. Do not end with only an `issue_progress_checkpoint`, final text, or an \"await next phase\" statement while the phase goal is still active.";
		let objective = match phase {
			PhaseGoalKind::ImplementToValidationReady => format!(
				"Decodex phase: {}\nProduce the smallest coherent implementation and documentation change for {} that is ready for the registered Decodex repo gate. Do not push, request review, or treat goal completion as issue completion. {phase_exit_contract}",
				phase.as_str(),
				self.issue_run.issue.identifier
			),
			PhaseGoalKind::RepairValidationFailures => format!(
				"Decodex phase: {}\nRepair repo-gate failures for {} in the current worktree without widening issue scope. {} {phase_exit_contract}",
				phase.as_str(),
				self.issue_run.issue.identifier,
				detail.unwrap_or("Run the registered canonicalize and verify commands before completing this phase.")
			),
			PhaseGoalKind::RepairAcceptedReviewFindings => format!(
				"Decodex phase: {}\nRepair accepted review findings for {} on the retained PR head without widening issue scope. Do not request GitHub Review before Decodex validation. {phase_exit_contract}",
				phase.as_str(),
				self.issue_run.issue.identifier
			),
			PhaseGoalKind::HandoffEvidence => format!(
				"Decodex phase: {}\nAfter Decodex validation, prepare PR-backed handoff evidence for {}: run the bounded review policy as instructed, push the branch when ready, create or update the non-draft PR, then record the required Decodex terminal path. Goal completion alone is not issue success.",
				phase.as_str(),
				self.issue_run.issue.identifier
			),
		};

		PhaseGoalSpec::new(phase, objective, None)
	}

	fn persist_next_phase_goal(&self, goal: &PhaseGoalSpec, reason: &str) -> Result<()> {
		self.state_store.append_private_execution_event(
			self.project.service_id(),
			&self.issue_run.issue.id,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
			"phase_goal_next",
			json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": goal.phase.as_str(),
				"reason": reason,
			}),
		)?;

		Ok(())
	}

	fn record_phase_goal_transition(
		&self,
		phase: PhaseGoalKind,
		signal: &str,
		payload: Value,
	) -> Result<()> {
		self.state_store.append_private_execution_event(
			self.project.service_id(),
			&self.issue_run.issue.id,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
			"phase_goal_transition",
			json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": phase.as_str(),
				"signal": signal,
				"payload": payload,
			}),
		)?;

		Ok(())
	}
}

impl PhaseGoalController for RepoGatePhaseGoalController<'_> {
	fn initial_phase_goal(&self) -> Result<Option<PhaseGoalSpec>> {
		if let Some(phase) = self.latest_persisted_phase_goal()? {
			return Ok(Some(self.phase_goal_spec(phase, None)));
		}

		Ok(Some(self.phase_goal_spec(self.initial_phase_goal_kind(), None)))
	}

	fn phase_goal_completed(&self, phase: PhaseGoalKind) -> Result<PhaseGoalTransition> {
		match phase {
			PhaseGoalKind::HandoffEvidence => {
				self.record_phase_goal_transition(
					phase,
					"handoff_evidence_goal_complete",
					json!({ "terminalPathRequired": true }),
				)?;

				Ok(PhaseGoalTransition::CompleteRun)
			},
			PhaseGoalKind::ImplementToValidationReady
			| PhaseGoalKind::RepairValidationFailures
			| PhaseGoalKind::RepairAcceptedReviewFindings => self.validate_phase_goal_output(phase),
		}
	}
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

struct ArchitectureRecoveryBoundary {
	disposition: AuthorityBoundaryDisposition,
	final_reason: &'static str,
	boundary_type: &'static str,
}

struct ArchitectureRecoveryPacketInput<'a> {
	project: &'a ServiceConfig,
	issue_run: &'a IssueRunPlan,
	loop_guardrail_stop: &'a LoopGuardrailStopRequested,
	error: &'a Report,
	contracts: &'a [DecisionContractRecord],
	boundary_check_record_id: i64,
	boundary_disposition: AuthorityBoundaryDisposition,
	boundary_final_reason: &'a str,
	reason_code: &'a str,
	recovery_attempt_number: usize,
	prior_started_count: usize,
}

struct ArchitectureRecoveryTerminalEventInput<'a> {
	project: &'a ServiceConfig,
	issue_run: &'a IssueRunPlan,
	stop: &'a LoopGuardrailStopRequested,
	boundary_check_record_id: i64,
	boundary_disposition: AuthorityBoundaryDisposition,
	reason_code: &'a str,
	recovery_attempt_number: usize,
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

#[derive(Clone, Copy, Eq, PartialEq)]
enum TerminalFailureEventRecordStatus {
	Recorded,
	Duplicate,
	NoLocalStore,
}

pub(crate) fn run_failure_writeback_disposition(
	error: &Report,
) -> RunFailureWritebackDisposition {
	if error.downcast_ref::<ManualAttentionRequested>().is_some()
		|| error.downcast_ref::<LoopGuardrailStopRequested>().is_some()
		|| error
			.downcast_ref::<AppServerPhaseGoalFailure>()
			.is_some_and(|failure| !failure.is_terminal_path_missing())
		|| error.downcast_ref::<ReviewHandoffNeedsAttention>().is_some()
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
		|| error
			.downcast_ref::<RepoGateFailure>()
			.is_some_and(|repo_gate_failure| {
				repo_gate_failure.disposition() == RepoGateFailureDisposition::NeedsHumanAttention
			})
	{
		RunFailureWritebackDisposition::TerminalAttention
	} else if error
		.downcast_ref::<AppServerZeroEvidenceStartFailure>()
		.is_some()
		|| error
			.downcast_ref::<AppServerCapabilityPreflightFailure>()
			.is_some_and(AppServerCapabilityPreflightFailure::is_retryable_timeout)
		|| error.downcast_ref::<StalledRunNeedsAttention>().is_some()
		|| error
		.downcast_ref::<RepoGateFailure>()
		.is_some_and(|repo_gate_failure| {
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

	let result = ensure_automation_activity_label(tracker, &issue_run.issue, project.service_id(), true)
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

fn configured_public_projection_privacy_classifier(
	project: &ServiceConfig,
) -> Result<tracker::privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier> {
	tracker::privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier::from_config(
		project.privacy_classifier(),
	)
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
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let body = format!("Decodex execution event: {}", record.event_type);
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, record, privacy_classifier)?;

	if state_store.record_linear_execution_event(&projection.record)?
		&& let Err(error) = tracker::create_prepared_linear_execution_event_comment_without_remote_scan(
			tracker,
			issue_id,
			&projection,
		)
	{
		state_store.forget_linear_execution_event(&projection.record.idempotency_key)?;

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
	let privacy_classifier = configured_public_projection_privacy_classifier(project)?;
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
		RunStartedLifecycleFields {
			worktree_path: &worktree_path,
			commit_sha: &commit_sha,
			privacy_classifier: &privacy_classifier,
		},
	)
}

fn write_run_started_lifecycle_event<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	fields: RunStartedLifecycleFields<'_>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let transport = workflow.frontmatter().agent().transport();
	let anchor = records::stable_event_anchor(&[
		issue_run.dispatch_mode.as_str(),
		&issue_run.worktree.branch_name,
		fields.commit_sha,
		transport,
	]);
	let mut record = records::LinearExecutionEventRecord::new(
		lifecycle_event_identity(project, issue_run),
		"run_started",
		current_timestamp(),
		&anchor,
	);

	record.branch = Some(issue_run.worktree.branch_name.clone());
	record.worktree_path = Some(fields.worktree_path.to_owned());
	record.commit_sha = Some(fields.commit_sha.to_owned());
	record.transport = Some(transport.to_owned());
	record.summary = Some(format!(
		"Decodex started a {} run for this issue.",
		issue_run.dispatch_mode.as_str()
	));

	write_lifecycle_event(
		tracker,
		state_store,
		&issue_run.issue.id,
		&record,
		fields.privacy_classifier,
	)
}

fn terminal_failure_lifecycle_event(
	service_id: &str,
	issue_run: &IssueRunPlan,
	failure: TerminalFailureLifecycle<'_>,
) -> records::LinearExecutionEventRecord {
	let retained_partial_progress = failure.error_class == "partial_progress_retained";
	let event_type = if failure.manual_attention_requested || retained_partial_progress {
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

	if retained_partial_progress {
		let mut evidence = vec![format!(
			"Attempt {} stopped with tracked worktree changes retained.",
			issue_run.attempt_number
		)];

		if let Some(source_error_class) = failure.retained_source_error_class {
			evidence.push(format!(
				"Source failure class `{source_error_class}` was preserved for recovery context."
			));
		}

		record.blockers = Some(vec![String::from(
			"Retained tracked worktree changes require operator recovery.",
		)]);
		record.evidence = Some(evidence);
		record.summary = Some(String::from("Decodex retained partial progress and needs attention."));
		record.terminal_path = Some(String::from("retained_partial_progress"));
	} else {
		record.blockers = Some(vec![format!("Run failed with `{}`.", failure.error_class)]);
		record.evidence = Some(vec![format!(
			"Attempt {} reached terminal failure handling.",
			issue_run.attempt_number
		)]);
		record.summary = Some(String::from("Decodex run failed and needs attention."));
	}

	record.pr_url = failure.pr_url.map(ToOwned::to_owned);
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
	let privacy_classifier = configured_public_projection_privacy_classifier(project)?;
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

	write_lifecycle_event(
		tracker,
		state_store,
		&issue_run.issue.id,
		&record,
		&privacy_classifier,
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
	let tracker_tool_bridge = TrackerToolBridge::with_run_context_state_store_and_privacy_classifier(
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
	let decodex_tool_bridge =
		DecodexToolBridge::new(&tracker_tool_bridge, build_decodex_run_context(workflow, issue_run));
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
			timeout: ACTIVE_RUN_IDLE_TIMEOUT,
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
				&& let Some(summary) = maybe_continue_after_active_phase_goal_recovery(
					input.project,
					input.workflow,
					input.state_store,
					input.issue_run,
					&error,
				)?
			{
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

fn build_phase_goal_controller<'a>(
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	issue_run: &'a IssueRunPlan,
) -> RepoGatePhaseGoalController<'a> {
	RepoGatePhaseGoalController {
		project,
		workflow,
		state_store,
		issue_run,
	}
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

fn maybe_continue_after_active_phase_goal_recovery(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<Option<RunSummary>> {
	let Some(recovery) = recover_active_phase_goal_continuation(
		project,
		workflow,
		state_store,
		issue_run,
		phase_goal_recovery_source_error_class(error),
	)? else {
		return Ok(None);
	};
	let mut summary = run_summary_from_issue_run(project.service_id(), issue_run);

	summary.continuation_pending = true;

	tracing::warn!(
		project_id = project.service_id(),
		issue_id = issue_run.issue.id,
		issue = issue_run.issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		source_phase = recovery.source_phase.as_str(),
		next_phase = recovery.next_phase.as_str(),
		error = %error,
		"Recovered active phase goal after app-server failure; scheduling continuation."
	);

	Ok(Some(summary))
}

fn recover_active_phase_goal_continuation(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	source_error_class: &str,
) -> Result<Option<PhaseGoalRecoveryContinuation>> {
	if !worktree_has_tracked_changes(&issue_run.worktree.path) {
		return Ok(None);
	}

	let Some(source_phase) = latest_active_phase_goal_recovery_candidate(
		project,
		state_store,
		issue_run,
	)? else {
		return Ok(None);
	};
	let controller = RepoGatePhaseGoalController {
		project,
		workflow,
		state_store,
		issue_run,
	};
	let transition = controller.validate_phase_goal_output(source_phase)?;
	let next_phase = match transition {
		PhaseGoalTransition::Continue(next_goal) => next_goal.phase,
		PhaseGoalTransition::CompleteRun => return Ok(None),
	};

	record_phase_goal_recovery_continuation(
		project,
		state_store,
		issue_run,
		source_phase,
		next_phase,
		source_error_class,
	)?;

	Ok(Some(PhaseGoalRecoveryContinuation { source_phase, next_phase }))
}

fn phase_goal_recovery_source_error_class(error: &Report) -> &'static str {
	retained_progress_source_error_class(error).unwrap_or("app_server_run_failed")
}

fn latest_active_phase_goal_recovery_candidate(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<Option<PhaseGoalKind>> {
	let events = state_store.list_private_execution_events(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.run_id,
		issue_run.attempt_number,
	)?;

	for event in events.iter().rev() {
		match event.event_type() {
			"phase_goal_completed"
			| "phase_goal_next"
			| "phase_goal_transition"
			| "review_completion_intent"
			| "terminal_finalize" => return Ok(None),
			"phase_goal_set" | "phase_goal_status" => {
				let Some(phase) = phase_goal_event_phase(event.payload()) else {
					return Ok(None);
				};
				let Some(status) = phase_goal_event_status(event.payload()) else {
					return Ok(None);
				};

				return Ok(phase_goal_recovery_candidate_from_status(phase, status));
			},
			_ => {},
		}
	}

	Ok(None)
}

fn phase_goal_event_phase(payload: &Value) -> Option<PhaseGoalKind> {
	payload
		.get("phase")
		.and_then(Value::as_str)
		.or_else(|| payload.get("payload")?.get("phase")?.as_str())
		.and_then(phase_goal_kind_from_str)
}

fn phase_goal_event_status(payload: &Value) -> Option<&str> {
	payload
		.get("status")
		.and_then(Value::as_str)
		.or_else(|| payload.get("payload")?.get("status")?.as_str())
}

fn phase_goal_recovery_candidate_from_status(
	phase: PhaseGoalKind,
	status: &str,
) -> Option<PhaseGoalKind> {
	if status != "active" {
		return None;
	}
	if matches!(
		phase,
		PhaseGoalKind::ImplementToValidationReady
			| PhaseGoalKind::RepairValidationFailures
			| PhaseGoalKind::RepairAcceptedReviewFindings
	) {
		Some(phase)
	} else {
		None
	}
}

fn record_phase_goal_recovery_continuation(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	source_phase: PhaseGoalKind,
	next_phase: PhaseGoalKind,
	source_error_class: &str,
) -> Result<()> {
	state_store.append_private_execution_event(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.run_id,
		issue_run.attempt_number,
		"phase_goal_recovery",
		json!({
			"schema": "decodex.phase_goal_signal/1",
			"phase": source_phase.as_str(),
			"signal": "active_goal_recovered",
			"payload": {
				"nextPhase": next_phase.as_str(),
				"sourceErrorClass": source_error_class,
			},
		}),
	)?;

	Ok(())
}

fn phase_goal_kind_from_str(value: &str) -> Option<PhaseGoalKind> {
	match value {
		"implement_to_validation_ready" => Some(PhaseGoalKind::ImplementToValidationReady),
		"repair_validation_failures" => Some(PhaseGoalKind::RepairValidationFailures),
		"repair_accepted_review_findings" => Some(PhaseGoalKind::RepairAcceptedReviewFindings),
		"handoff_evidence" => Some(PhaseGoalKind::HandoffEvidence),
		_ => None,
	}
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
	let candidates = match terminal_thread_archive_backlog_candidates(state_store, project.service_id())
	{
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
			run_completion_repo_gate(workflow, issue_run)?;

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

fn retryable_failure_loop_guardrail_stop(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<Option<LoopGuardrailStopRequested>> {
	let Some(worktree_fingerprint) =
		loop_guardrail_worktree_fingerprint(&issue_run.worktree.path)?
	else {
		return Ok(None);
	};
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
				"{}:{}:{}",
				worktree_fingerprint.head_sha.as_str(),
				worktree_fingerprint.effective_status_hash.as_str(),
				worktree_fingerprint.tracked_diff_hash.as_str()
			),
			retained_progress_source_error_class(error),
		));
	}

	for (reason, fingerprint, source_error_class) in observations {
		let checkpoint = state_store.observe_loop_guardrail_checkpoint(
			LoopGuardrailCheckpointInput {
				project_id: project.service_id(),
				issue_id: &issue_run.issue.id,
				reason: reason.error_class(),
				fingerprint: &fingerprint,
				run_id: &issue_run.run_id,
				attempt_number: issue_run.attempt_number,
				details_json: &json!({
					"schema": "decodex.loop_guardrail_checkpoint/1",
					"reason": reason.error_class(),
					"source_error_class": source_error_class,
					"head_sha": worktree_fingerprint.head_sha.as_str(),
					"tracked_status_hash": worktree_fingerprint.tracked_status_hash.as_str(),
					"tracked_diff_hash": worktree_fingerprint.tracked_diff_hash.as_str(),
					"effective_status_hash": worktree_fingerprint.effective_status_hash.as_str(),
					"effective_delta_present": worktree_fingerprint.effective_delta_present,
					"threshold": LOOP_GUARDRAIL_CONVERGENCE_BUDGET,
				})
				.to_string(),
			},
		)?;

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

	Ok(Some(LoopGuardrailWorktreeFingerprint {
		head_sha,
		tracked_status_hash: loop_guardrail_text_hash(&tracked_status),
		tracked_diff_hash: loop_guardrail_text_hash(&tracked_diff),
		effective_status_hash: loop_guardrail_text_hash(&effective_status),
		effective_delta_present: !effective_status.trim().is_empty()
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
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(args)
		.output()?;

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
		fingerprint: format!(
			"{}:{}",
			review_policy_stop.head_sha,
			review_policy_stop.nonclean_rounds.unwrap_or_default()
		),
		source_error_class: Some(review_policy_stop.reason.error_class().to_owned()),
		architecture_recovery_reason_code: None,
	}
}

fn loop_guardrail_architecture_recovery_decision(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	mut loop_guardrail_stop: LoopGuardrailStopRequested,
	error: &Report,
) -> Result<LoopGuardrailRecoveryDecision> {
	let prior_started_count =
		architecture_recovery_started_count(state_store, project, issue_run)?;
	let recovery_attempt_number = prior_started_count.saturating_add(1);
	let boundary = classify_loop_guardrail_authority_boundary(&loop_guardrail_stop, error);
	let contracts = architecture_recovery_contracts_for_issue(state_store, project, issue_run)?;
	let decision_contract_ids =
		contracts.iter().map(|contract| contract.contract_id().to_owned()).collect::<Vec<_>>();
	let decision_contract_id_refs =
		decision_contract_ids.iter().map(String::as_str).collect::<Vec<_>>();
	let boundary_event = record_authority_boundary_check_private_event(
		state_store,
		AuthorityBoundaryCheckInput {
			project_id: project.service_id(),
			issue_id: &issue_run.issue.id,
			issue_identifier: &issue_run.issue.identifier,
			run_id: &issue_run.run_id,
			attempt_number: issue_run.attempt_number,
			decision_contract_ids: decision_contract_id_refs,
			attempted_recovery_reason: loop_guardrail_stop.reason.error_class(),
			changed_surfaces: architecture_recovery_changed_surfaces(&boundary),
			disposition: boundary.disposition,
			final_disposition_reason: boundary.final_reason,
			improvement_signals: architecture_recovery_improvement_signals(
				loop_guardrail_stop.reason,
				&boundary,
			),
		},
	)?;
	let budget_exhausted = prior_started_count >= ARCHITECTURE_RECOVERY_BUDGET;
	let reason_code = architecture_recovery_reason_code(&boundary, budget_exhausted);

	record_architecture_recovery_packet(
		state_store,
		ArchitectureRecoveryPacketInput {
			project,
			issue_run,
			loop_guardrail_stop: &loop_guardrail_stop,
			error,
			contracts: &contracts,
			boundary_check_record_id: boundary_event.record_id(),
			boundary_disposition: boundary.disposition,
			boundary_final_reason: boundary.final_reason,
			reason_code,
			recovery_attempt_number,
			prior_started_count,
		},
	)?;

	if budget_exhausted || boundary.disposition != AuthorityBoundaryDisposition::WithinAuthority {
		loop_guardrail_stop.architecture_recovery_reason_code = Some(reason_code.to_owned());

		record_architecture_recovery_terminal_event(
			state_store,
			ArchitectureRecoveryTerminalEventInput {
				project,
				issue_run,
				stop: &loop_guardrail_stop,
				boundary_check_record_id: boundary_event.record_id(),
				boundary_disposition: boundary.disposition,
				reason_code,
				recovery_attempt_number,
			},
		)?;

		if boundary.disposition != AuthorityBoundaryDisposition::WithinAuthority {
			let decision_request_id = format!(
				"{}-{}-{}-{}",
				issue_run.issue.identifier,
				issue_run.run_id,
				issue_run.attempt_number,
				reason_code
			);

			record_authority_decision_request_private_event(
				state_store,
				architecture_recovery_decision_request_input(
					project,
					issue_run,
					&loop_guardrail_stop,
					boundary_event.record_id(),
					&decision_request_id,
					reason_code,
					boundary.final_reason,
				),
			)?;
		}

		return Ok(LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop));
	}

	state_store.clear_loop_guardrail_checkpoint(
		project.service_id(),
		&issue_run.issue.id,
		loop_guardrail_stop.reason.error_class(),
	)?;

	record_architecture_recovery_started_event(
		state_store,
		project,
		issue_run,
		&loop_guardrail_stop,
		boundary_event.record_id(),
		recovery_attempt_number,
	)?;

	Ok(LoopGuardrailRecoveryDecision::Start(ArchitectureRecoveryStart {
		attempt_number: recovery_attempt_number,
		max_attempts: ARCHITECTURE_RECOVERY_BUDGET,
		detail: architecture_recovery_goal_detail(&loop_guardrail_stop, recovery_attempt_number),
	}))
}

fn classify_loop_guardrail_authority_boundary(
	stop: &LoopGuardrailStopRequested,
	error: &Report,
) -> ArchitectureRecoveryBoundary {
	let source_is_repo_gate =
		stop.source_error_class.as_deref().is_some_and(|class| class.starts_with("repo_gate_"))
			|| error.downcast_ref::<RepoGateFailure>().is_some_and(|failure| {
				failure.disposition() == RepoGateFailureDisposition::ContinueRepair
			});

	match stop.reason {
		LoopGuardrailReason::ValidationRepeat | LoopGuardrailReason::RemainingDeltaUnchanged
			if source_is_repo_gate =>
		{
			ArchitectureRecoveryBoundary {
				disposition: AuthorityBoundaryDisposition::WithinAuthority,
				final_reason: "Repo-gate convergence failed on an engineering implementation problem; architecture recovery may change implementation strategy without weakening validation.",
				boundary_type: "implementation_strategy",
			}
		},
		LoopGuardrailReason::NoEffectiveDiff if source_is_repo_gate => {
			ArchitectureRecoveryBoundary {
				disposition: AuthorityBoundaryDisposition::WithinAuthority,
				final_reason: "No-effective-diff convergence followed repo-gate repair work; architecture recovery may replace the ineffective implementation strategy.",
				boundary_type: "implementation_strategy",
			}
		},
		LoopGuardrailReason::ReviewChurn => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::WithinAuthority,
			final_reason: "Review churn can be recovered autonomously only by changing implementation architecture while preserving accepted behavior and review standards.",
			boundary_type: "implementation_strategy",
		},
		LoopGuardrailReason::DependencyProgramStale => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			final_reason: "The next viable action changes dependency or Execution Program readiness and requires accepted authority.",
			boundary_type: "external_dependency",
		},
		LoopGuardrailReason::UncoveredDirection => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			final_reason: "Execution uncovered missing direction that changes the accepted Decision Contract.",
			boundary_type: "decision_contract",
		},
		LoopGuardrailReason::AmbiguousRetainedProgress => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::InsufficientEvidence,
			final_reason: "Retained progress ownership is underspecified, so Decodex lacks evidence that recovery is inside authority.",
			boundary_type: "retained_ownership",
		},
		_ => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::InsufficientEvidence,
			final_reason: "Guardrail evidence is insufficient to prove autonomous recovery stays inside the Authority Envelope.",
			boundary_type: "authority_evidence",
		},
	}
}

fn architecture_recovery_started_count(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
) -> Result<usize> {
	Ok(state_store
		.list_private_execution_events_for_issue(project.service_id(), &issue_run.issue.id)?
		.iter()
		.filter(|event| event.event_type() == ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE)
		.count())
}

fn architecture_recovery_contracts_for_issue(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
) -> Result<Vec<DecisionContractRecord>> {
	let mut records = Vec::new();

	for issue_id in [&issue_run.issue.id, &issue_run.issue.identifier] {
		for record in state_store.list_decision_contracts_for_issue(project.service_id(), issue_id)? {
			if records
				.iter()
				.all(|existing: &DecisionContractRecord| existing.contract_id() != record.contract_id())
			{
				records.push(record);
			}
		}
	}

	records.sort_by(|left, right| left.contract_id().cmp(right.contract_id()));

	Ok(records)
}

fn architecture_recovery_changed_surfaces(
	boundary: &ArchitectureRecoveryBoundary,
) -> Vec<AuthorityBoundaryChangedSurface<'static>> {
	vec![AuthorityBoundaryChangedSurface {
		surface: boundary.boundary_type,
		change_summary: "Replace the non-converging guardrail repair strategy with a materially different architecture recovery strategy.",
		classification: boundary.disposition,
	}]
}

fn architecture_recovery_improvement_signals(
	reason: LoopGuardrailReason,
	boundary: &ArchitectureRecoveryBoundary,
) -> Vec<AuthorityBoundaryImprovementSignal<'static>> {
	match boundary.disposition {
		AuthorityBoundaryDisposition::WithinAuthority => match reason {
			LoopGuardrailReason::ValidationRepeat | LoopGuardrailReason::RemainingDeltaUnchanged => {
				vec![AuthorityBoundaryImprovementSignal {
					kind: "missing_validator",
					reason_code: "validation_guardrail_repeated",
					target: "validator:repo_gate",
					recommendation: "Promote the repeated repo-gate failure into an earlier deterministic validator or fixture.",
				}]
			},
			_ => vec![AuthorityBoundaryImprovementSignal {
				kind: "weak_prompt",
				reason_code: "architecture_recovery_strategy_needed",
				target: "prompt:phase_goal_repair",
				recommendation: "Prompt recovery agents to replace the ineffective strategy instead of repeating patch-only repair.",
			}],
		},
		AuthorityBoundaryDisposition::RequiresHuman => vec![AuthorityBoundaryImprovementSignal {
			kind: "underspecified_decision_contract",
			reason_code: "contract_boundary_required",
			target: "decision_contract:authority_envelope",
			recommendation: "Record explicit accepted authority before retrying autonomous recovery.",
		}],
		AuthorityBoundaryDisposition::InsufficientEvidence => {
			vec![AuthorityBoundaryImprovementSignal {
				kind: "underspecified_decision_contract",
				reason_code: "authority_evidence_missing",
				target: "issue_template:loop_recovery",
				recommendation: "Capture retained ownership, validation, and Decision Contract evidence before recovery.",
			}]
		},
	}
}

fn architecture_recovery_reason_code(
	boundary: &ArchitectureRecoveryBoundary,
	budget_exhausted: bool,
) -> &'static str {
	if budget_exhausted {
		"architecture_recovery_exhausted"
	} else if boundary.boundary_type == "external_dependency" {
		"external_dependency_required"
	} else if boundary.disposition == AuthorityBoundaryDisposition::WithinAuthority {
		"architecture_recovery_started"
	} else {
		"contract_boundary_required"
	}
}

fn record_architecture_recovery_packet(
	state_store: &StateStore,
	input: ArchitectureRecoveryPacketInput<'_>,
) -> Result<()> {
	let programs = architecture_recovery_programs_for_contracts(
		state_store,
		input.project.service_id(),
		input.contracts,
	)?;
	let retained = architecture_recovery_retained_worktree(&input.issue_run.worktree.path)?;
	let review = architecture_recovery_review_findings(state_store, input.project, input.issue_run)?;

	state_store
		.append_private_execution_event(
			input.project.service_id(),
			&input.issue_run.issue.id,
			&input.issue_run.run_id,
			input.issue_run.attempt_number,
			ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE,
			json!({
				"schema": ARCHITECTURE_RECOVERY_PACKET_SCHEMA,
				"record_version": 1,
				"state": input.reason_code,
				"reason_code": input.reason_code,
				"issue": architecture_recovery_issue_payload(input.issue_run),
				"run": architecture_recovery_run_payload(input.issue_run),
				"decision_contract_context": input.contracts
					.iter()
					.map(architecture_recovery_contract_payload)
					.collect::<Vec<_>>(),
				"execution_program_context": programs
					.iter()
					.map(architecture_recovery_program_payload)
					.collect::<Vec<_>>(),
				"retained_worktree": retained,
				"validation_failures": architecture_recovery_validation_failures(
					input.loop_guardrail_stop,
					input.error,
				),
				"review_findings": review,
				"prior_recovery_attempts": {
					"started_count": input.prior_started_count,
				},
				"recovery_budget": {
					"attempt": input.recovery_attempt_number,
					"max_attempts": ARCHITECTURE_RECOVERY_BUDGET,
				},
				"loop_guardrail": {
					"reason": input.loop_guardrail_stop.reason.error_class(),
					"consecutive_count": input.loop_guardrail_stop.consecutive_count,
					"threshold": LOOP_GUARDRAIL_CONVERGENCE_BUDGET,
					"fingerprint": input.loop_guardrail_stop.fingerprint.as_str(),
					"source_error_class": input.loop_guardrail_stop.source_error_class.as_deref(),
				},
				"authority_boundary_check": {
					"record_id": input.boundary_check_record_id,
					"disposition": input.boundary_disposition.as_str(),
					"reason": input.boundary_final_reason,
				},
			}),
		)
		.map(|_| ())
}

fn architecture_recovery_programs_for_contracts(
	state_store: &StateStore,
	project_id: &str,
	contracts: &[DecisionContractRecord],
) -> Result<Vec<ExecutionProgramRecord>> {
	let mut programs = Vec::new();

	for contract in contracts {
		for program in state_store
			.list_execution_programs_for_contract(project_id, contract.contract_id())?
		{
			if programs
				.iter()
				.all(|existing: &ExecutionProgramRecord| existing.program_id() != program.program_id())
			{
				programs.push(program);
			}
		}
	}

	programs.sort_by(|left, right| left.program_id().cmp(right.program_id()));

	Ok(programs)
}

fn architecture_recovery_retained_worktree(worktree_path: &Path) -> Result<Value> {
	let fingerprint = loop_guardrail_worktree_fingerprint(worktree_path)?;
	let tracked_status =
		git_guardrail_output(worktree_path, &["status", "--porcelain", "--untracked-files=no"])?;
	let raw_status = git_guardrail_output(worktree_path, &["status", "--porcelain"])?;
	let effective_status = raw_status.as_deref().map(loop_guardrail_effective_status);
	let diff_stat =
		git_guardrail_output(worktree_path, &["diff", "--stat", "--no-ext-diff", "HEAD", "--"])?;

	Ok(json!({
		"head_sha": fingerprint.as_ref().map(|value| value.head_sha.as_str()),
		"tracked_status_hash": fingerprint
			.as_ref()
			.map(|value| value.tracked_status_hash.as_str()),
		"tracked_diff_hash": fingerprint.as_ref().map(|value| value.tracked_diff_hash.as_str()),
		"effective_status_hash": fingerprint
			.as_ref()
			.map(|value| value.effective_status_hash.as_str()),
		"effective_delta_present": fingerprint
			.as_ref()
			.map(|value| value.effective_delta_present),
		"tracked_status": tracked_status.unwrap_or_default(),
		"effective_status": effective_status.unwrap_or_default(),
		"diff_stat": diff_stat.unwrap_or_default(),
	}))
}

fn architecture_recovery_review_findings(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
) -> Result<Value> {
	let events = state_store.list_private_execution_events_for_issue(
		project.service_id(),
		&issue_run.issue.id,
	)?;
	let latest_review = events
		.iter()
		.rev()
		.find(|event| event.event_type() == "review_checkpoint")
		.map(|event| event.payload());
	let Some(payload) = latest_review else {
		return Ok(json!({
			"latest_status": null,
			"accepted_finding_count": 0,
			"rejected_finding_count": 0,
		}));
	};
	let review = payload.get("review").unwrap_or(payload);

	Ok(json!({
		"latest_status": payload.get("status").and_then(Value::as_str),
		"accepted_finding_count": review
			.get("accepted_findings")
			.and_then(Value::as_array)
			.map_or(0, Vec::len),
		"rejected_finding_count": review
			.get("rejected_findings")
			.and_then(Value::as_array)
			.map_or(0, Vec::len),
		"nonclean_rounds": payload.get("nonclean_rounds").and_then(Value::as_i64).unwrap_or(0),
	}))
}

fn architecture_recovery_issue_payload(issue_run: &IssueRunPlan) -> Value {
	json!({
		"id": issue_run.issue.id.as_str(),
		"identifier": issue_run.issue.identifier.as_str(),
		"title": issue_run.issue.title.as_str(),
	})
}

fn architecture_recovery_run_payload(issue_run: &IssueRunPlan) -> Value {
	json!({
		"run_id": issue_run.run_id.as_str(),
		"attempt_number": issue_run.attempt_number,
		"branch": issue_run.worktree.branch_name.as_str(),
		"dispatch_mode": issue_run.dispatch_mode.as_str(),
	})
}

fn architecture_recovery_contract_payload(record: &DecisionContractRecord) -> Value {
	json!({
		"contract_id": record.contract_id(),
		"source_issue_id": record.source_issue_id(),
		"status": record.status().as_str(),
		"updated_at": record.updated_at(),
	})
}

fn architecture_recovery_program_payload(record: &ExecutionProgramRecord) -> Value {
	json!({
		"program_id": record.program_id(),
		"source_contract_id": record.source_contract_id(),
		"updated_at": record.updated_at(),
	})
}

fn architecture_recovery_validation_failures(
	stop: &LoopGuardrailStopRequested,
	error: &Report,
) -> Value {
	json!({
		"guardrail_reason": stop.reason.error_class(),
		"source_error_class": stop.source_error_class.as_deref(),
		"error_summary": truncate_private_diagnostic_text(&error.to_string()),
	})
}

fn record_architecture_recovery_started_event(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
	stop: &LoopGuardrailStopRequested,
	boundary_check_record_id: i64,
	recovery_attempt_number: usize,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE,
			json!({
				"schema": "decodex.architecture_recovery_started/1",
				"record_version": 1,
				"reason_code": "architecture_recovery_started",
				"guardrail_reason": stop.reason.error_class(),
				"authority_boundary_check_record_id": boundary_check_record_id,
				"recovery_budget": {
					"attempt": recovery_attempt_number,
					"max_attempts": ARCHITECTURE_RECOVERY_BUDGET,
				},
				"next_strategy": "materially_different_architecture_recovery",
			}),
		)
		.map(|_| ())
}

fn record_architecture_recovery_terminal_event(
	state_store: &StateStore,
	input: ArchitectureRecoveryTerminalEventInput<'_>,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			input.project.service_id(),
			&input.issue_run.issue.id,
			&input.issue_run.run_id,
			input.issue_run.attempt_number,
			ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE,
			json!({
				"schema": "decodex.architecture_recovery_terminal/1",
				"record_version": 1,
				"reason_code": input.reason_code,
				"guardrail_reason": input.stop.reason.error_class(),
				"authority_boundary_check_record_id": input.boundary_check_record_id,
				"boundary_disposition": input.boundary_disposition.as_str(),
				"recovery_budget": {
					"attempt": input.recovery_attempt_number,
					"max_attempts": ARCHITECTURE_RECOVERY_BUDGET,
				},
			}),
		)
		.map(|_| ())
}

fn architecture_recovery_decision_request_input<'a>(
	project: &'a ServiceConfig,
	issue_run: &'a IssueRunPlan,
	stop: &'a LoopGuardrailStopRequested,
	boundary_check_record_id: i64,
	decision_request_id: &'a str,
	reason_code: &'a str,
	final_reason: &'a str,
) -> AuthorityDecisionRequestInput<'a> {
	AuthorityDecisionRequestInput {
		project_id: project.service_id(),
		issue_id: &issue_run.issue.id,
		issue_identifier: &issue_run.issue.identifier,
		run_id: &issue_run.run_id,
		attempt_number: issue_run.attempt_number,
		boundary_check_record_id,
		decision_request_id,
		reason_code,
		boundary_type: "architecture_recovery",
		proposed_change: "Continue loop recovery with a materially different architecture strategy.",
		why_exceeds_authority: final_reason,
		options: vec![
			AuthorityDecisionOption {
				label: "Authorize recovery",
				description: "Update the issue, Decision Contract, or policy to allow this recovery.",
			},
			AuthorityDecisionOption {
				label: "Keep stopped",
				description: "Leave the lane in manual attention until the boundary is resolved.",
			},
		],
		recommendation: "Resolve the authority boundary before requeueing the lane.",
		resume_condition: "Accept, reject, or revise the requested authority in the issue, Decision Contract, or project policy before clearing needs-attention.",
		retained_worktree_evidence: vec![issue_run.worktree.branch_name.as_str()],
		retained_diff_evidence: vec![stop.fingerprint.as_str()],
		recovery_attempt_context: vec![stop.reason.error_class()],
	}
}

fn architecture_recovery_goal_detail(
	stop: &LoopGuardrailStopRequested,
	recovery_attempt_number: usize,
) -> String {
	format!(
		"Loop guardrail `{}` stopped the current ineffective strategy after {} matching observations. Decodex recorded an Architecture Recovery Packet and an Authority Boundary Check with `within_authority`; use autonomous architecture recovery attempt {} of {}. Start a materially different implementation strategy, preserve the accepted Decision Contract and all validation/review gates, and request human attention only if the next viable action would change product behavior, public API/config contract, security, data, credential, billing, validation standards, or accepted authority.",
		stop.reason.error_class(),
		stop.consecutive_count,
		recovery_attempt_number,
		ARCHITECTURE_RECOVERY_BUDGET
	)
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
	let retry_budget_attempts =
		retry_budget_attempts_for_current_failure(state_store, issue_run)?;
	let failure_context = FailureHandlingContext {
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		worktree_path: &worktree_path,
		retry_budget_attempts,
	};
	let loop_guardrail_stop = if requires_terminal_attention {
		None
	} else {
		retryable_failure_loop_guardrail_stop(project, state_store, issue_run, error)?
	};
	let retained_partial_progress = retained_partial_progress_error(
		error,
		issue_run,
		&worktree_path,
	);

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
				apply_architecture_recovery_retry_writeback(&failure_context, recovery, max_attempts),
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
				apply_architecture_recovery_retry_writeback(&failure_context, recovery, max_attempts),
			LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop) =>
				apply_loop_guardrail_failure_writeback(&failure_context, loop_guardrail_stop),
		};
	}

	if !requires_terminal_attention && retry_budget_attempts < max_attempts {
		return apply_retryable_failure_writeback(&failure_context, error, max_attempts);
	}

	let terminal_error = retained_partial_progress.as_ref().unwrap_or(error);
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
		Some("failed"),
		None,
	);

	Ok(())
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
			next_action: "decodex recorded a within-authority boundary check and will retry with a materially different architecture recovery strategy",
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
	} else if let Some(app_server_failure) =
		error.downcast_ref::<AppServerCapabilityPreflightFailure>()
	{
		Some(app_server_failure.error_class())
	} else if let Some(app_server_failure) =
		error.downcast_ref::<AppServerHomePreflightFailure>()
	{
		Some(app_server_failure.error_class())
	} else if let Some(account_failure) = error.downcast_ref::<CodexAccountAuthFailure>() {
		Some(account_failure.error_class())
	} else if let Some(app_server_failure) =
		error.downcast_ref::<AppServerTransportFailure>()
	{
		Some(app_server_failure.error_class())
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerPhaseGoalFailure>() {
		Some(app_server_failure.error_class())
	} else if let Some(app_server_failure) =
		error.downcast_ref::<AppServerDynamicToolFailure>()
	{
		Some(app_server_failure.error_class())
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerTurnFailure>() {
		Some(app_server_failure.error_class())
	} else if let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() {
		Some(repo_gate_failure.error_class())
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

	write_retry_schedule_marker(
		workflow,
		issue_run,
		retry_budget_attempts,
		retry_kind,
	)
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
