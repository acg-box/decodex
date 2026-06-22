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
const REVIEW_HANDOFF_STATE_DRIFT_DETECTED_EVENT_TYPE: &str =
	"review_handoff_state_drift_detected";
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

struct AgentGitCredentialEnvironment {
	process_env: AppServerProcessEnv,
}
impl AgentGitCredentialEnvironment {
	fn process_env(&self) -> &AppServerProcessEnv {
		&self.process_env
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
	policy_decision: AuthorityBoundaryPolicyDecision,
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
			IssueDispatchMode::Normal | IssueDispatchMode::Program | IssueDispatchMode::Retry =>
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

	fn latest_cross_attempt_phase_goal(&self) -> Result<Option<PhaseGoalKind>> {
		if !matches!(
			self.issue_run.dispatch_mode,
			IssueDispatchMode::Normal | IssueDispatchMode::Program | IssueDispatchMode::Retry
		) {
			return Ok(None);
		}

		latest_open_issue_phase_goal_before_attempt(
			self.project,
			self.state_store,
			&self.issue_run.issue.id,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
		)
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
				let acceptance_check =
					self.evaluate_phase_acceptance(phase, &selected_repo_gate)?;

				self.record_phase_acceptance_check(&acceptance_check)?;

				if acceptance_check.decision == PhaseAcceptanceDecision::Fail {
					return self.continue_after_phase_acceptance_failure(phase, &acceptance_check);
				}

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
		let phase_exit_contract = "Phase exit contract: before completing this phase, record a current-HEAD `issue_progress_checkpoint` with `docs_impact` set to `none`, `update_required`, `research_required`, or `drift_required`; when this phase objective is satisfied, explicitly mark the active phase goal complete with the Codex goal completion mechanism so Decodex can run its repo gate and select the next phase. Do not end with only an `issue_progress_checkpoint`, final text, or an \"await next phase\" statement while the phase goal is still active.";
		let objective = match phase {
			PhaseGoalKind::ImplementToValidationReady => format!(
				"Decodex phase: {}\nProduce the smallest coherent implementation and documentation change for {} that is ready for the registered Decodex repo gate, including docs impact classification recorded as `docs_impact` in a current-HEAD `issue_progress_checkpoint`. Do not push, request review, or treat goal completion as issue completion. {phase_exit_contract}",
				phase.as_str(),
				self.issue_run.issue.identifier
			),
			PhaseGoalKind::RepairValidationFailures => format!(
				"Decodex phase: {}\nRepair repo-gate failures for {} in the current worktree without widening issue scope, including any required docs impact update or drift evidence. {} {phase_exit_contract}",
				phase.as_str(),
				self.issue_run.issue.identifier,
				detail.unwrap_or("Run the registered canonicalize and verify commands before completing this phase.")
			),
			PhaseGoalKind::RepairAcceptedReviewFindings => format!(
				"Decodex phase: {}\nRepair accepted review findings for {} on the retained PR head without widening issue scope, including any required docs impact update or drift evidence. Do not request GitHub Review before Decodex validation. {phase_exit_contract}",
				phase.as_str(),
				self.issue_run.issue.identifier
			),
			PhaseGoalKind::HandoffEvidence => format!(
				"Decodex phase: {}\nAfter Decodex validation, prepare PR-backed handoff evidence for {}: record a current-HEAD `issue_progress_checkpoint` with `docs_impact`, run the bounded review policy as instructed, push the branch when ready, create or update the non-draft PR, then record the required Decodex terminal path. Goal completion alone is not issue success.",
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

	fn evaluate_phase_acceptance(
		&self,
		phase: PhaseGoalKind,
		repo_gate: &ResolvedRepoGate<'_>,
	) -> Result<PhaseAcceptanceCheck> {
		let fingerprint = loop_guardrail_worktree_fingerprint(&self.issue_run.worktree.path)?;
		let head_sha = fingerprint.as_ref().map(|value| value.head_sha.clone());
		let changed_surfaces = phase_acceptance_changed_surfaces(&self.issue_run.worktree.path);
		let effective_delta_present = fingerprint
			.as_ref()
			.is_some_and(|value| value.effective_delta_present)
			|| !changed_surfaces.is_empty();
		let checkpoint = self.latest_progress_checkpoint()?;
		let checkpoint_payload = checkpoint.as_ref().map(state::PrivateExecutionEvent::payload);
		let checkpoint_head_sha = checkpoint_payload
			.and_then(|payload| payload.get("head_sha"))
			.and_then(Value::as_str)
			.map(str::to_owned);
		let checkpoint_matches_head = head_sha
			.as_deref()
			.zip(checkpoint_head_sha.as_deref())
			.is_some_and(|(head, checkpoint_head)| head == checkpoint_head);
		let docs_impact_valid = checkpoint_payload
			.and_then(|payload| payload.get("docs_impact"))
			.and_then(Value::as_str)
			.is_some_and(phase_acceptance_docs_impact_valid);
		let blocker_count = checkpoint_payload.map_or(0, phase_acceptance_blocker_count);
		let non_goal_violation = checkpoint_payload.is_some_and(phase_acceptance_has_non_goal_violation);
		let objective_covered =
			checkpoint.is_some() && checkpoint_matches_head && docs_impact_valid && blocker_count == 0;
		let non_goal_passed = !non_goal_violation;
		let validation_passed = true;
		let reason_code = phase_acceptance_reason_code(
			checkpoint.is_some(),
			checkpoint_matches_head,
			docs_impact_valid,
			effective_delta_present,
			non_goal_passed,
			blocker_count,
		);
		let decision = if reason_code == "accepted" {
			PhaseAcceptanceDecision::Pass
		} else {
			PhaseAcceptanceDecision::Fail
		};

		Ok(PhaseAcceptanceCheck {
			phase,
			decision,
			reason_code,
			objective_covered,
			effective_delta_present,
			changed_surfaces,
			non_goal_passed,
			validation_passed,
			repo_gate_profile: repo_gate.profile_name().map(str::to_owned),
			canonicalize_commands: repo_gate.canonicalize_commands().to_vec(),
			verify_commands: repo_gate.verify_commands().to_vec(),
			checkpoint_record_id: checkpoint.as_ref().map(state::PrivateExecutionEvent::record_id),
			checkpoint_head_sha,
			worktree_head_sha: head_sha,
			blocker_count,
		})
	}

	fn latest_progress_checkpoint(&self) -> Result<Option<state::PrivateExecutionEvent>> {
		let events = self.state_store.list_private_execution_events(
			self.project.service_id(),
			&self.issue_run.issue.id,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
		)?;

		Ok(events
			.into_iter()
				.rev()
				.find(|event| event.event_type() == "progress_checkpoint"))
	}

	fn continue_after_phase_acceptance_failure(
		&self,
		phase: PhaseGoalKind,
		acceptance_check: &PhaseAcceptanceCheck,
	) -> Result<PhaseGoalTransition> {
		let failure = PhaseAcceptanceCheckFailure::new(acceptance_check.reason_code);
		let error_class = failure.error_class();
		let error = Report::new(failure);

		self.record_phase_goal_transition(
			phase,
			"validation_fail",
			json!({
				"errorClass": error_class,
				"disposition": RepoGateFailureDisposition::ContinueRepair.as_str(),
				"acceptanceDecision": acceptance_check.decision.as_str(),
				"acceptanceReason": acceptance_check.reason_code,
			}),
		)?;

		if let Some(loop_guardrail_stop) =
			retryable_failure_loop_guardrail_stop(self.project, self.state_store, self.issue_run, &error)?
		{
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

		let next_phase = phase_acceptance_repair_phase(phase);
		let detail = format!(
			"Phase acceptance check failed after repo gate pass with `{}`. {}",
			acceptance_check.reason_code,
			acceptance_check.next_action()
		);
		let next_goal = self.phase_goal_spec(next_phase, Some(&detail));

		self.persist_next_phase_goal(&next_goal, "phase_acceptance_fail")?;

		Ok(PhaseGoalTransition::Continue(next_goal))
	}

	fn record_phase_acceptance_check(&self, check: &PhaseAcceptanceCheck) -> Result<()> {
		self.state_store.append_private_execution_event(
			self.project.service_id(),
			&self.issue_run.issue.id,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
			PHASE_ACCEPTANCE_CHECK_EVENT_TYPE,
			json!({
				"schema": "decodex.phase_acceptance_check/1",
				"phase": check.phase.as_str(),
				"decision": check.decision.as_str(),
				"reason_code": check.reason_code,
				"objective_coverage": {
					"covered": check.objective_covered,
					"checkpoint_record_id": check.checkpoint_record_id,
					"checkpoint_head_sha": check.checkpoint_head_sha.as_deref(),
					"worktree_head_sha": check.worktree_head_sha.as_deref(),
				},
				"effective_delta": {
					"present": check.effective_delta_present,
					"changed_surfaces": &check.changed_surfaces,
				},
				"non_goal_check": {
					"passed": check.non_goal_passed,
					"blocker_count": check.blocker_count,
				},
				"validation_evidence": {
					"repo_gate_passed": check.validation_passed,
					"repo_gate_profile": check.repo_gate_profile.as_deref(),
					"canonicalize_commands": &check.canonicalize_commands,
					"verify_commands": &check.verify_commands,
				},
				"next_action": check.next_action(),
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
		if let Some(phase) = self.latest_cross_attempt_phase_goal()? {
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

#[derive(Debug)]
struct PhaseAcceptanceCheckFailure {
	reason_code: String,
}
impl PhaseAcceptanceCheckFailure {
	fn new(reason_code: impl Into<String>) -> Self {
		Self { reason_code: reason_code.into() }
	}

	fn error_class(&self) -> &'static str {
		"phase_acceptance_check_failed"
	}
}

impl Display for PhaseAcceptanceCheckFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "Phase acceptance check failed: {}", self.reason_code)
	}
}

impl Error for PhaseAcceptanceCheckFailure {}

struct PhaseAcceptanceCheck {
	phase: PhaseGoalKind,
	decision: PhaseAcceptanceDecision,
	reason_code: &'static str,
	objective_covered: bool,
	effective_delta_present: bool,
	changed_surfaces: Vec<String>,
	non_goal_passed: bool,
	validation_passed: bool,
	repo_gate_profile: Option<String>,
	canonicalize_commands: Vec<String>,
	verify_commands: Vec<String>,
	checkpoint_record_id: Option<i64>,
	checkpoint_head_sha: Option<String>,
	worktree_head_sha: Option<String>,
	blocker_count: usize,
}
impl PhaseAcceptanceCheck {
	fn next_action(&self) -> &'static str {
		match self.reason_code {
			"accepted" => "continue to handoff evidence",
			"missing_progress_checkpoint" => {
				"record a current-HEAD issue_progress_checkpoint with docs_impact before completing the phase goal again"
			},
			"stale_progress_checkpoint" => {
				"record a fresh issue_progress_checkpoint for the current worktree HEAD before completing the phase goal again"
			},
			"docs_impact_missing" => {
				"record parseable docs_impact in the current-HEAD issue_progress_checkpoint"
			},
			"no_effective_delta" => {
				"produce an issue-scoped effective delta before completing the phase goal again"
			},
			"non_goal_violation" => {
				"remove or explicitly resolve the non-goal or scope violation before handoff"
			},
			"progress_blockers_present" => {
				"clear recorded progress blockers or route to manual attention before handoff"
			},
			_ => "inspect phase_acceptance_check evidence before selecting the next phase",
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

struct ArchitectureRecoveryBoundary {
	disposition: AuthorityBoundaryDisposition,
	policy_decision: AuthorityBoundaryPolicyDecision,
	final_reason: &'static str,
	boundary_type: AuthorityBoundarySurface,
}

struct ArchitectureRecoveryPacketInput<'a> {
	project: &'a ServiceConfig,
	issue_run: &'a IssueRunPlan,
	loop_guardrail_stop: &'a LoopGuardrailStopRequested,
	error: &'a Report,
	contracts: &'a [DecisionContractRecord],
	boundary_check_record_id: i64,
	boundary_disposition: AuthorityBoundaryDisposition,
	boundary_policy_decision: AuthorityBoundaryPolicyDecision,
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
	boundary_policy_decision: AuthorityBoundaryPolicyDecision,
	boundary_final_reason: &'a str,
	reason_code: &'a str,
	recovery_attempt_number: usize,
}

#[derive(Clone, Copy)]
struct PhaseGoalRecoveryRecord<'a> {
	project: &'a ServiceConfig,
	state_store: &'a StateStore,
	issue_run: &'a IssueRunPlan,
	source_phase: PhaseGoalKind,
	next_phase: PhaseGoalKind,
	source_error_class: &'a str,
	source_error_message: Option<&'a str>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PhaseAcceptanceDecision {
	Pass,
	Fail,
}
impl PhaseAcceptanceDecision {
	fn as_str(self) -> &'static str {
		match self {
			Self::Pass => "pass",
			Self::Fail => "fail",
		}
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

fn phase_acceptance_reason_code(
	checkpoint_present: bool,
	checkpoint_matches_head: bool,
	docs_impact_valid: bool,
	effective_delta_present: bool,
	non_goal_passed: bool,
	blocker_count: usize,
) -> &'static str {
	if !checkpoint_present {
		return "missing_progress_checkpoint";
	}
	if !checkpoint_matches_head {
		return "stale_progress_checkpoint";
	}
	if !docs_impact_valid {
		return "docs_impact_missing";
	}
	if !effective_delta_present {
		return "no_effective_delta";
	}
	if !non_goal_passed {
		return "non_goal_violation";
	}
	if blocker_count > 0 {
		return "progress_blockers_present";
	}

	"accepted"
}

fn phase_acceptance_repair_phase(phase: PhaseGoalKind) -> PhaseGoalKind {
	match phase {
		PhaseGoalKind::RepairAcceptedReviewFindings => PhaseGoalKind::RepairAcceptedReviewFindings,
		PhaseGoalKind::ImplementToValidationReady
		| PhaseGoalKind::RepairValidationFailures
		| PhaseGoalKind::HandoffEvidence => PhaseGoalKind::RepairValidationFailures,
	}
}

fn phase_acceptance_changed_surfaces(worktree_path: &Path) -> Vec<String> {
	let mut surfaces = BTreeSet::new();

	if let Ok(changed_files) = repo_gate_changed_tracked_files(worktree_path) {
		surfaces.extend(changed_files);
	}
	if let Ok(Some(diff_paths)) =
		git_guardrail_output(worktree_path, &["diff", "--name-only", "--diff-filter=ACDMRTUXB", "HEAD", "--"])
	{
		for path in diff_paths.lines().map(str::trim).filter(|path| !path.is_empty()) {
			surfaces.insert(path.to_owned());
		}
	}
	if let Ok(Some(status)) =
		git_guardrail_output(worktree_path, &["status", "--porcelain"])
	{
		for surface in status.lines().filter_map(phase_acceptance_status_surface) {
			surfaces.insert(surface);
		}
	}

	surfaces.into_iter().collect()
}

fn phase_acceptance_status_surface(line: &str) -> Option<String> {
	let line = line.trim_end();

	if line.is_empty() || state::is_untracked_decodex_runtime_artifact_status_line(line) {
		return None;
	}

	let path = line.get(3..)?.trim();
	let path = path.rsplit_once(" -> ").map_or(path, |(_, renamed_path)| renamed_path.trim());

	(!path.is_empty()).then(|| path.to_owned())
}

fn phase_acceptance_blocker_count(payload: &Value) -> usize {
	payload
		.get("blockers")
		.and_then(Value::as_array)
		.map_or(0, Vec::len)
}

fn phase_acceptance_docs_impact_valid(value: &str) -> bool {
	matches!(value, "none" | "update_required" | "research_required" | "drift_required")
}

fn phase_acceptance_has_non_goal_violation(payload: &Value) -> bool {
	payload
		.get("blockers")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.any(|blocker| {
			let normalized = blocker.to_ascii_lowercase();

			normalized.contains("non-goal")
				|| normalized.contains("non_goal")
				|| normalized.contains("out of scope")
				|| normalized.contains("scope violation")
		})
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

fn maybe_continue_after_phase_goal_recovery(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<Option<RunSummary>> {
	let source_error_message = phase_goal_recovery_source_error_message(error);
	let Some(recovery) = recover_phase_goal_continuation(
		project,
		workflow,
		state_store,
		issue_run,
		phase_goal_recovery_source_error_class(error),
		Some(source_error_message.as_str()),
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
		"Recovered phase goal after app-server failure; scheduling continuation."
	);

	Ok(Some(summary))
}

fn recover_phase_goal_continuation(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	source_error_class: &str,
	source_error_message: Option<&str>,
) -> Result<Option<PhaseGoalRecoveryContinuation>> {
	if !worktree_has_tracked_changes(&issue_run.worktree.path) {
		return Ok(None);
	}

	let Some(source_phase) = latest_phase_goal_recovery_candidate(
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
	let prior_recovery_count = matching_phase_goal_recovery_count(
		project,
		state_store,
		issue_run,
		source_phase,
		source_error_class,
	)?;
	let recovery_record = PhaseGoalRecoveryRecord {
		project,
		state_store,
		issue_run,
		source_phase,
		next_phase,
		source_error_class,
		source_error_message,
	};

	if prior_recovery_count >= PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT {
		record_phase_goal_recovery_blocked(recovery_record, prior_recovery_count)?;

		return Ok(None);
	}

	record_phase_goal_recovery_continuation(recovery_record)?;

	Ok(Some(PhaseGoalRecoveryContinuation { source_phase, next_phase }))
}

fn phase_goal_recovery_source_error_class(error: &Report) -> &'static str {
	retained_progress_source_error_class(error).unwrap_or("app_server_run_failed")
}

fn phase_goal_recovery_source_error_message(error: &Report) -> String {
	truncate_phase_goal_recovery_error(error.to_string(), 512)
}

fn truncate_phase_goal_recovery_error(value: String, max_chars: usize) -> String {
	if value.chars().count() <= max_chars {
		return value;
	}

	let mut truncated = value.chars().take(max_chars).collect::<String>();

	truncated.push_str("...");

	truncated
}

fn latest_phase_goal_recovery_candidate(
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
			AUTHORITY_DECISION_REQUEST_EVENT_TYPE => return Ok(None),
			"progress_checkpoint" if progress_checkpoint_has_blockers(event.payload()) =>
				return Ok(None),
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

fn latest_open_issue_phase_goal_before_attempt(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_id: &str,
	current_run_id: &str,
	current_attempt_number: i64,
) -> Result<Option<PhaseGoalKind>> {
	if current_attempt_number <= 1 {
		return Ok(None);
	}

	let events = state_store.list_private_execution_events_for_issue(
		project.service_id(),
		issue_id,
	)?;

	for event in events.iter().rev().filter(|event| {
		event.attempt_number() < current_attempt_number && event.run_id() != current_run_id
	}) {
		match event.event_type() {
			"terminal_finalize"
			| "review_completion_intent"
			| AUTHORITY_DECISION_REQUEST_EVENT_TYPE
			| PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE
			| RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE => return Ok(None),
			"progress_checkpoint" if progress_checkpoint_has_blockers(event.payload()) =>
				return Ok(None),
			PHASE_GOAL_RECOVERY_EVENT_TYPE | "phase_goal_next" | "phase_goal_transition" => {
				if let Some(phase) =
					phase_goal_continuation_next_phase(event.event_type(), event.payload())
				{
					return Ok(Some(phase));
				}
			},
			"phase_goal_set" | "phase_goal_status" => {
				if let Some(phase) = phase_goal_active_phase(event.payload()) {
					return Ok(Some(phase));
				}
			},
			_ => {},
		}
	}

	Ok(None)
}

fn progress_checkpoint_has_blockers(payload: &Value) -> bool {
	payload.get("blockers").is_some_and(|blockers| match blockers {
		Value::Array(items) => !items.is_empty(),
		Value::Null => false,
		_ => true,
	})
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

fn phase_goal_active_phase(payload: &Value) -> Option<PhaseGoalKind> {
	let phase = phase_goal_event_phase(payload)?;
	let status = phase_goal_event_status(payload)?;

	(status == "active").then_some(phase)
}

fn matching_phase_goal_recovery_count(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	source_phase: PhaseGoalKind,
	source_error_class: &str,
) -> Result<i64> {
	let events =
		state_store.list_private_execution_events_for_issue(project.service_id(), &issue_run.issue.id)?;

	Ok(events
		.iter()
		.filter(|event| {
			event.event_type() == PHASE_GOAL_RECOVERY_EVENT_TYPE
				&& phase_goal_recovery_event_source_phase(event.payload())
					.is_some_and(|phase| phase == source_phase.as_str())
				&& phase_goal_recovery_event_source_error_class(event.payload())
					.is_some_and(|class| class == source_error_class)
		})
		.count() as i64)
}

fn phase_goal_recovery_event_source_phase(payload: &Value) -> Option<&str> {
	payload
		.get("phase")
		.and_then(Value::as_str)
		.or_else(|| payload.get("payload")?.get("sourcePhase")?.as_str())
}

fn phase_goal_recovery_event_source_error_class(payload: &Value) -> Option<&str> {
	payload.get("payload")?.get("sourceErrorClass")?.as_str()
}

fn phase_goal_continuation_next_phase(
	event_type: &str,
	payload: &Value,
) -> Option<PhaseGoalKind> {
	let phase = if event_type == "phase_goal_next" {
		payload.get("phase")?.as_str()?
	} else {
		payload.get("payload")?.get("nextPhase")?.as_str()?
	};

	phase_goal_kind_from_str(phase)
}

fn record_phase_goal_recovery_continuation(record: PhaseGoalRecoveryRecord<'_>) -> Result<()> {
	record.state_store.append_private_execution_event(
		record.project.service_id(),
		&record.issue_run.issue.id,
		&record.issue_run.run_id,
		record.issue_run.attempt_number,
		PHASE_GOAL_RECOVERY_EVENT_TYPE,
		json!({
			"schema": "decodex.phase_goal_signal/1",
			"phase": record.source_phase.as_str(),
			"signal": "phase_goal_recovered",
			"payload": {
				"nextPhase": record.next_phase.as_str(),
				"sourceErrorClass": record.source_error_class,
				"sourceErrorMessage": record.source_error_message,
			},
		}),
	)?;

	Ok(())
}

fn record_phase_goal_recovery_blocked(
	record: PhaseGoalRecoveryRecord<'_>,
	prior_recovery_count: i64,
) -> Result<()> {
	record.state_store.append_private_execution_event(
		record.project.service_id(),
		&record.issue_run.issue.id,
		&record.issue_run.run_id,
		record.issue_run.attempt_number,
		PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE,
		json!({
			"schema": "decodex.phase_goal_signal/1",
			"phase": record.source_phase.as_str(),
			"signal": "continuation_budget_exhausted",
			"payload": {
				"nextPhase": record.next_phase.as_str(),
				"sourceErrorClass": record.source_error_class,
				"sourceErrorMessage": record.source_error_message,
				"priorRecoveryCount": prior_recovery_count,
				"automaticContinuationLimit": PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT,
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

	let Some(worktree_fingerprint) =
		loop_guardrail_worktree_fingerprint(&issue_run.worktree.path)?
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

	if let ReviewHandoffStateDriftTransition::MoveToSuccess(state_id) =
		success_state_transition
	{
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

	Ok(Some(ReviewHandoffStateDriftTransition::MoveToSuccess(
		state_id.to_owned(),
	)))
}

fn rebound_review_handoff_orchestration_marker(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	review_handoff: &ReviewHandoffMarker,
	local_head_sha: &str,
) -> Result<bool> {
	let existing_orchestration =
		state_store.review_orchestration_marker(project.service_id(), &issue_run.issue.id, review_handoff)?;
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
		existing_orchestration
			.as_ref()
			.map_or(0, ReviewOrchestrationMarker::external_round_count),
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

	let Some(worktree_fingerprint) =
		loop_guardrail_worktree_fingerprint(&issue_run.worktree.path)?
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

			if checkpoint.status() != "clean" || checkpoint.head_sha() != worktree_fingerprint.head_sha
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
				"{}:{}",
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
					"branch_delta_present": worktree_fingerprint.branch_delta_present,
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
	let branch_delta_present =
		repo_gate_changed_tracked_files(worktree_path).is_ok_and(|changed_files| !changed_files.is_empty());

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
	let changed_surfaces =
		architecture_recovery_changed_surfaces(&boundary, &issue_run.worktree.path);
	let policy_decision = architecture_recovery_policy_decision(&changed_surfaces);
	let disposition = policy_decision.disposition();
	let final_reason = architecture_recovery_final_reason(&boundary, policy_decision);
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
			changed_surfaces,
			policy_decision,
			disposition,
			final_disposition_reason: final_reason,
			improvement_signals: architecture_recovery_improvement_signals(
				loop_guardrail_stop.reason,
				&boundary,
			),
		},
	)?;
	let budget_exhausted = prior_started_count >= ARCHITECTURE_RECOVERY_BUDGET;
	let reason_code =
		architecture_recovery_reason_code(&boundary, policy_decision, budget_exhausted);

	record_architecture_recovery_packet(
		state_store,
		ArchitectureRecoveryPacketInput {
			project,
			issue_run,
			loop_guardrail_stop: &loop_guardrail_stop,
			error,
			contracts: &contracts,
			boundary_check_record_id: boundary_event.record_id(),
			boundary_disposition: disposition,
			boundary_policy_decision: policy_decision,
			boundary_final_reason: final_reason,
			reason_code,
			recovery_attempt_number,
			prior_started_count,
		},
	)?;

	if budget_exhausted || !policy_decision.allows_autonomous_recovery() {
		loop_guardrail_stop.architecture_recovery_reason_code = Some(reason_code.to_owned());

		record_architecture_recovery_terminal_outcome(
			state_store,
			ArchitectureRecoveryTerminalEventInput {
				project,
				issue_run,
				stop: &loop_guardrail_stop,
				boundary_check_record_id: boundary_event.record_id(),
				boundary_disposition: disposition,
				boundary_policy_decision: policy_decision,
				boundary_final_reason: final_reason,
				reason_code,
				recovery_attempt_number,
			},
		)?;

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
		policy_decision,
		recovery_attempt_number,
	)?;

	Ok(LoopGuardrailRecoveryDecision::Start(ArchitectureRecoveryStart {
		attempt_number: recovery_attempt_number,
		max_attempts: ARCHITECTURE_RECOVERY_BUDGET,
		policy_decision,
		detail: architecture_recovery_goal_detail(
			&loop_guardrail_stop,
			recovery_attempt_number,
			policy_decision,
		),
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
				policy_decision: AuthorityBoundaryPolicyDecision::AutoContinue,
				final_reason: "Repo-gate convergence failed on an engineering implementation problem; architecture recovery may change implementation strategy without weakening validation.",
				boundary_type: AuthorityBoundarySurface::ImplementationStrategy,
			}
		},
		LoopGuardrailReason::NoEffectiveDiff if source_is_repo_gate => {
			ArchitectureRecoveryBoundary {
				disposition: AuthorityBoundaryDisposition::WithinAuthority,
				policy_decision: AuthorityBoundaryPolicyDecision::AutoContinue,
				final_reason: "No-effective-diff convergence followed repo-gate repair work; architecture recovery may replace the ineffective implementation strategy.",
				boundary_type: AuthorityBoundarySurface::ImplementationStrategy,
			}
		},
		LoopGuardrailReason::ReviewChurn => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::WithinAuthority,
			policy_decision: AuthorityBoundaryPolicyDecision::BlockLanding,
			final_reason: "Review churn can be recovered autonomously only by changing implementation architecture while preserving accepted behavior and review standards.",
			boundary_type: AuthorityBoundarySurface::ReviewPolicy,
		},
		LoopGuardrailReason::DependencyProgramStale => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			final_reason: "The next viable action changes dependency or Execution Program readiness and requires accepted authority.",
			boundary_type: AuthorityBoundarySurface::ExternalDependency,
		},
		LoopGuardrailReason::UncoveredDirection => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			final_reason: "Execution uncovered missing direction that changes the accepted Decision Contract.",
			boundary_type: AuthorityBoundarySurface::Objective,
		},
		LoopGuardrailReason::AmbiguousRetainedProgress => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			final_reason: "Retained progress ownership is underspecified, so Decodex lacks evidence that recovery is inside authority.",
			boundary_type: AuthorityBoundarySurface::RetainedOwnership,
		},
		_ => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			final_reason: "Guardrail evidence is insufficient to prove autonomous recovery stays inside the Authority Envelope.",
			boundary_type: AuthorityBoundarySurface::AuthorityEvidence,
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
	worktree_path: &Path,
) -> Vec<AuthorityBoundaryChangedSurface<'static>> {
	let mut surfaces = Vec::new();

	push_architecture_recovery_changed_surface(
		&mut surfaces,
		boundary.boundary_type,
		"Replace the non-converging guardrail repair strategy with a materially different architecture recovery strategy.",
		boundary.policy_decision,
		boundary.disposition,
	);

	if let Ok(Some(diff_paths)) =
		git_guardrail_output(worktree_path, &["diff", "--name-only", "HEAD", "--"])
	{
		for relative_path in diff_paths.lines().filter(|path| !path.trim().is_empty()) {
			for surface in architecture_recovery_surfaces_for_path(relative_path) {
				push_architecture_recovery_changed_surface(
					&mut surfaces,
					surface,
					architecture_recovery_surface_summary(surface),
					surface.policy_decision(),
					surface.policy_decision().disposition(),
				);
			}
		}
	}

	surfaces
}

fn push_architecture_recovery_changed_surface(
	surfaces: &mut Vec<AuthorityBoundaryChangedSurface<'static>>,
	surface: AuthorityBoundarySurface,
	change_summary: &'static str,
	policy_decision: AuthorityBoundaryPolicyDecision,
	legacy_disposition: AuthorityBoundaryDisposition,
) {
	if surfaces.iter().any(|existing| existing.surface == surface) {
		return;
	}

	surfaces.push(AuthorityBoundaryChangedSurface {
		surface,
		change_summary,
		policy_decision,
		legacy_disposition,
	});
}

fn architecture_recovery_surfaces_for_path(
	relative_path: &str,
) -> Vec<AuthorityBoundarySurface> {
	let normalized = relative_path.replace('\\', "/");
	let lower = normalized.to_ascii_lowercase();
	let mut surfaces = Vec::new();

	if lower.starts_with("docs/") {
		surfaces.push(AuthorityBoundarySurface::Docs);

		return surfaces;
	}
	if architecture_recovery_path_is_test(&lower) {
		surfaces.push(AuthorityBoundarySurface::Tests);

		return surfaces;
	}
	if architecture_recovery_path_is_config(&lower) {
		surfaces.push(AuthorityBoundarySurface::Config);

		return surfaces;
	}
	if architecture_recovery_path_is_public_api(&lower) {
		surfaces.push(AuthorityBoundarySurface::PublicApi);
	}
	if architecture_recovery_path_is_security(&lower) {
		surfaces.push(AuthorityBoundarySurface::Security);
	}
	if architecture_recovery_path_is_privacy(&lower) {
		surfaces.push(AuthorityBoundarySurface::Privacy);
	}
	if architecture_recovery_path_is_data(&lower) {
		surfaces.push(AuthorityBoundarySurface::Data);
	}
	if architecture_recovery_path_is_billing(&lower) {
		surfaces.push(AuthorityBoundarySurface::Billing);
	}
	if architecture_recovery_path_is_validation(&lower) {
		surfaces.push(AuthorityBoundarySurface::Validation);
	}
	if architecture_recovery_path_is_review_policy(&lower) {
		surfaces.push(AuthorityBoundarySurface::ReviewPolicy);
	}
	if surfaces.is_empty() && architecture_recovery_path_is_runtime(&lower) {
		surfaces.push(AuthorityBoundarySurface::Runtime);
	}

	surfaces
}

fn architecture_recovery_path_is_test(path: &str) -> bool {
	path.starts_with("tests/")
		|| path.contains("/tests/")
		|| path.ends_with("_test.rs")
		|| path.ends_with("tests.rs")
		|| path.contains("/test_")
}

fn architecture_recovery_path_is_config(path: &str) -> bool {
	path == "cargo.toml"
		|| path == "cargo.lock"
		|| path == "makefile.toml"
		|| path == "clippy.toml"
		|| path == "rust-toolchain.toml"
		|| path == "decodex.example.toml"
		|| path.starts_with(".github/")
		|| path.ends_with(".toml")
		|| path.ends_with(".yaml")
		|| path.ends_with(".yml")
		|| path.ends_with(".json")
		|| path.ends_with(".env")
}

fn architecture_recovery_path_is_public_api(path: &str) -> bool {
	architecture_recovery_path_has_segment(path, "cli")
		|| architecture_recovery_path_has_segment(path, "mcp")
		|| architecture_recovery_path_has_segment(path, "protocol")
		|| architecture_recovery_path_has_segment(path, "api")
		|| path.contains("tracker_tool_bridge")
		|| path.contains("app_bridge")
}

fn architecture_recovery_path_is_security(path: &str) -> bool {
	path.contains("auth")
		|| path.contains("credential")
		|| path.contains("secret")
		|| path.contains("security")
		|| path.contains("signing")
		|| path.contains("token")
}

fn architecture_recovery_path_is_privacy(path: &str) -> bool {
	path.contains("privacy") || path.contains("public_text") || path.contains("redact")
}

fn architecture_recovery_path_is_data(path: &str) -> bool {
	path.contains("database")
		|| path.contains("migration")
		|| path.contains("payload")
		|| path.contains("record")
		|| path.contains("sqlite")
		|| path.contains("state")
}

fn architecture_recovery_path_is_billing(path: &str) -> bool {
	path.contains("account")
		|| path.contains("billing")
		|| path.contains("credit")
		|| path.contains("invoice")
		|| path.contains("usage")
}

fn architecture_recovery_path_is_validation(path: &str) -> bool {
	path.contains("repo_gate")
		|| path.contains("validation")
		|| path.contains("validator")
		|| path.contains("verify")
}

fn architecture_recovery_path_is_review_policy(path: &str) -> bool {
	path.contains("review_policy") || path.contains("review_landing") || path.contains("landing")
}

fn architecture_recovery_path_is_runtime(path: &str) -> bool {
	path.starts_with("apps/") || path.starts_with("scripts/") || path.starts_with("dev/")
}

fn architecture_recovery_path_has_segment(path: &str, segment: &str) -> bool {
	path.split('/').any(|part| {
		part == segment
			|| part
				.strip_suffix(".rs")
				.is_some_and(|stem| stem == segment)
	})
}

fn architecture_recovery_surface_summary(surface: AuthorityBoundarySurface) -> &'static str {
	match surface {
		AuthorityBoundarySurface::ImplementationStrategy => {
			"Replace the non-converging guardrail repair strategy with a materially different architecture recovery strategy."
		},
		AuthorityBoundarySurface::Runtime => {
			"Runtime implementation files changed during recovery."
		},
		AuthorityBoundarySurface::Tests => "Test files changed during recovery.",
		AuthorityBoundarySurface::Docs => "Documentation files changed during recovery.",
		AuthorityBoundarySurface::PublicApi => {
			"Public API or command surface files changed during recovery."
		},
		AuthorityBoundarySurface::Config => "Configuration files changed during recovery.",
		AuthorityBoundarySurface::Security => {
			"Security-sensitive implementation files changed during recovery."
		},
		AuthorityBoundarySurface::Data => "Data or state persistence files changed during recovery.",
		AuthorityBoundarySurface::Billing => "Billing or usage files changed during recovery.",
		AuthorityBoundarySurface::Privacy => "Privacy-sensitive files changed during recovery.",
		AuthorityBoundarySurface::Validation => {
			"Validation or repository-gate files changed during recovery."
		},
		AuthorityBoundarySurface::ReviewPolicy => {
			"Review policy or landing policy files changed during recovery."
		},
		AuthorityBoundarySurface::Objective => {
			"Objective-changing recovery requires an explicit human decision."
		},
		AuthorityBoundarySurface::NonGoal => {
			"Non-goal-changing recovery requires an explicit human decision."
		},
		AuthorityBoundarySurface::ExternalDependency => {
			"External dependency recovery requires accepted authority."
		},
		AuthorityBoundarySurface::RetainedOwnership => {
			"Retained ownership evidence changed during recovery."
		},
		AuthorityBoundarySurface::AuthorityEvidence => {
			"Authority evidence changed or is insufficient during recovery."
		},
	}
}

fn architecture_recovery_policy_decision(
	surfaces: &[AuthorityBoundaryChangedSurface<'_>],
) -> AuthorityBoundaryPolicyDecision {
	surfaces.iter().fold(AuthorityBoundaryPolicyDecision::AutoContinue, |decision, surface| {
		AuthorityBoundaryPolicyDecision::max(decision, surface.policy_decision)
	})
}

fn architecture_recovery_final_reason(
	boundary: &ArchitectureRecoveryBoundary,
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> &'static str {
	if policy_decision == boundary.policy_decision {
		return boundary.final_reason;
	}

	match policy_decision {
		AuthorityBoundaryPolicyDecision::AutoContinue => boundary.final_reason,
		AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence => {
			"Changed high-risk surfaces can continue recovery autonomously, but require enhanced evidence before review handoff or landing."
		},
		AuthorityBoundaryPolicyDecision::BlockLanding => {
			"Changed validation or review-policy surfaces can continue recovery autonomously, but block landing until the required evidence is restored."
		},
		AuthorityBoundaryPolicyDecision::RequiresHumanDecision => boundary.final_reason,
	}
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
	policy_decision: AuthorityBoundaryPolicyDecision,
	budget_exhausted: bool,
) -> &'static str {
	if budget_exhausted {
		"architecture_recovery_exhausted"
	} else if boundary.boundary_type == AuthorityBoundarySurface::ExternalDependency {
		"external_dependency_required"
	} else if policy_decision.allows_autonomous_recovery() {
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
					"policy_decision": input.boundary_policy_decision.as_str(),
					"requires_enhanced_evidence": input
						.boundary_policy_decision
						.requires_enhanced_evidence(),
					"blocks_landing": input.boundary_policy_decision.blocks_landing(),
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
	let route_summary = review.get("finding_route_summary");

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
		"route_counts": route_summary
			.and_then(|summary| summary.get("route_counts"))
			.cloned()
			.unwrap_or_else(|| json!([])),
		"route_next_action": route_summary
			.and_then(|summary| summary.get("next_action"))
			.and_then(Value::as_str),
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
	boundary_policy_decision: AuthorityBoundaryPolicyDecision,
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
				"boundary_policy_decision": boundary_policy_decision.as_str(),
				"requires_enhanced_evidence": boundary_policy_decision.requires_enhanced_evidence(),
				"blocks_landing": boundary_policy_decision.blocks_landing(),
				"recovery_budget": {
					"attempt": recovery_attempt_number,
					"max_attempts": ARCHITECTURE_RECOVERY_BUDGET,
				},
				"next_strategy": "materially_different_architecture_recovery",
			}),
		)
		.map(|_| ())
}

fn record_architecture_recovery_terminal_outcome(
	state_store: &StateStore,
	input: ArchitectureRecoveryTerminalEventInput<'_>,
) -> Result<()> {
	record_architecture_recovery_terminal_event(state_store, &input)?;

	if input.boundary_policy_decision.allows_autonomous_recovery() {
		return Ok(());
	}

	let decision_request_id = format!(
		"{}-{}-{}-{}",
		input.issue_run.issue.identifier,
		input.issue_run.run_id,
		input.issue_run.attempt_number,
		input.reason_code
	);

	record_authority_decision_request_private_event(
		state_store,
		architecture_recovery_decision_request_input(
			input.project,
			input.issue_run,
			input.stop,
			input.boundary_check_record_id,
			&decision_request_id,
			input.reason_code,
			input.boundary_final_reason,
		),
	)
	.map(|_| ())
}

fn record_architecture_recovery_terminal_event(
	state_store: &StateStore,
	input: &ArchitectureRecoveryTerminalEventInput<'_>,
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
				"boundary_policy_decision": input.boundary_policy_decision.as_str(),
				"requires_enhanced_evidence": input
					.boundary_policy_decision
					.requires_enhanced_evidence(),
				"blocks_landing": input.boundary_policy_decision.blocks_landing(),
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
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> String {
	format!(
		"Loop guardrail `{}` stopped the current ineffective strategy after {} matching observations. Decodex recorded an Architecture Recovery Packet and an Authority Boundary Check with policy `{}`; use autonomous architecture recovery attempt {} of {}. Start a materially different implementation strategy, preserve the accepted Decision Contract and all validation/review gates, and {}.",
		stop.reason.error_class(),
		stop.consecutive_count,
		policy_decision.as_str(),
		recovery_attempt_number,
		ARCHITECTURE_RECOVERY_BUDGET,
		architecture_recovery_policy_recovery_guidance(policy_decision)
	)
}

fn architecture_recovery_policy_recovery_guidance(
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> &'static str {
	match policy_decision {
		AuthorityBoundaryPolicyDecision::AutoContinue => {
			"request human attention only if the next viable action would change product behavior, public API/config contract, security, data, credential, billing, validation standards, or accepted authority"
		},
		AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence => {
			"preserve enhanced evidence for the changed high-risk surfaces before review handoff or landing"
		},
		AuthorityBoundaryPolicyDecision::BlockLanding => {
			"keep landing blocked until validation or review-policy evidence is restored"
		},
		AuthorityBoundaryPolicyDecision::RequiresHumanDecision => {
			"request human attention before continuing recovery"
		},
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

	let Some(attention_error) =
		review_handoff_state_drift_attention_error(project, workflow, state_store, issue_run, error)?
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
	let failure_state_is_startable = tracker_policy
		.startable_states()
		.iter()
		.any(|state| state == failure_state_name);

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
	if context
		.state_store
		.issue_has_review_lifecycle_record(context.project.service_id(), &context.issue_run.issue.id)?
	{
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
	error
		.downcast_ref::<AppServerZeroEvidenceStartFailure>()
		.is_some()
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

fn architecture_recovery_retry_next_action(
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> &'static str {
	match policy_decision {
		AuthorityBoundaryPolicyDecision::AutoContinue => {
			"decodex recorded authority policy `auto_continue` and will retry with a materially different architecture recovery strategy"
		},
		AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence => {
			"decodex recorded authority policy `requires_enhanced_evidence` and will retry with a materially different architecture recovery strategy while preserving enhanced evidence before review handoff or landing"
		},
		AuthorityBoundaryPolicyDecision::BlockLanding => {
			"decodex recorded authority policy `block_landing` and will retry with a materially different architecture recovery strategy while landing remains blocked until validation or review-policy evidence is restored"
		},
		AuthorityBoundaryPolicyDecision::RequiresHumanDecision => {
			"decodex recorded authority policy `requires_human_decision` and requires human attention before retrying"
		},
	}
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
	} else if let Some(acceptance_failure) =
		error.downcast_ref::<PhaseAcceptanceCheckFailure>()
	{
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
