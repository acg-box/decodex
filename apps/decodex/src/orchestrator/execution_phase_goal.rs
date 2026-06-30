//! Phase-goal controller, acceptance checks, and continuation recovery.

use super::*;

pub(super) struct PhaseGoalRecoveryContinuation {
	pub(super) source_phase: PhaseGoalKind,
	pub(super) next_phase: PhaseGoalKind,
}

pub(super) struct RepoGatePhaseGoalController<'a> {
	pub(super) project: &'a ServiceConfig,
	pub(super) workflow: &'a WorkflowDocument,
	pub(super) state_store: &'a StateStore,
	pub(super) issue_run: &'a IssueRunPlan,
}
impl RepoGatePhaseGoalController<'_> {
	fn initial_phase_goal_kind(&self) -> PhaseGoalKind {
		match self.issue_run.dispatch_mode {
			IssueDispatchMode::Normal | IssueDispatchMode::Program | IssueDispatchMode::Retry => {
				PhaseGoalKind::ImplementToValidationReady
			},
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

		match run_repo_gate_commands_allow_owned_tracked_rewrites(
			selected_repo_gate.canonicalize_commands(),
			selected_repo_gate.verify_commands(),
			&self.issue_run.worktree.path,
		) {
			Ok(repo_gate_outcome) => {
				let acceptance_check =
					self.evaluate_phase_acceptance(phase, &selected_repo_gate, &repo_gate_outcome)?;

				self.record_phase_acceptance_check(&acceptance_check)?;

				if acceptance_check.decision == PhaseAcceptanceDecision::Fail {
					return self.continue_after_phase_acceptance_failure(phase, &acceptance_check);
				}

				self.state_store.clear_loop_guardrail_checkpoints_for_issue(
					self.project.service_id(),
					&self.issue_run.issue.id,
				)?;

				let next_phase = phase_validation_pass_next_phase(phase);
				let mut transition_payload = json!({ "nextPhase": next_phase.as_str() });

				if let Some(decision) = repo_gate_outcome.tracked_rewrite_decision() {
					transition_payload["trackedRewrites"] = decision.to_json();
				}

				self.record_phase_goal_transition(phase, "validation_pass", transition_payload)?;

				let handoff_detail = repo_gate_outcome
					.tracked_rewrite_decision()
					.map(|decision| phase_tracked_rewrite_handoff_detail(next_phase, decision));
				let next_goal = self.phase_goal_spec(next_phase, handoff_detail.as_deref());

				self.persist_next_phase_goal(&next_goal, "validation_pass")?;

				Ok(PhaseGoalTransition::Continue(next_goal))
			},
			Err(error) => {
				if let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() {
					let mut transition_payload = json!({
						"errorClass": repo_gate_failure.error_class(),
						"disposition": repo_gate_failure.disposition().as_str(),
					});

					if let Some(diagnostic) = repo_gate_failure.diagnostic() {
						transition_payload["repoGateFailure"] = diagnostic.to_json();
					}
					if let Some(decision) = repo_gate_failure.tracked_rewrite_decision() {
						transition_payload["trackedRewrites"] = decision.to_json();
					}

					self.record_phase_goal_transition(
						phase,
						"validation_fail",
						transition_payload,
					)?;

					if repo_gate_failure.disposition() == RepoGateFailureDisposition::ContinueRepair
					{
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

									self.persist_next_phase_goal(
										&next_goal,
										"architecture_recovery_started",
									)?;

									return Ok(PhaseGoalTransition::Continue(next_goal));
								},
								LoopGuardrailRecoveryDecision::HumanRequired(
									loop_guardrail_stop,
								) => {
									return Err(Report::new(loop_guardrail_stop).wrap_err(error));
								},
							}
						}

						let detail = format!(
							"{} Inspect the worktree, run the registered canonicalize and verify commands, and repair only the validation failure.",
							repo_gate_failure.repair_target_detail()
						);
						let next_goal = self.phase_goal_spec(
							PhaseGoalKind::RepairValidationFailures,
							Some(&detail),
						);

						self.persist_next_phase_goal(&next_goal, "validation_fail")?;

						return Ok(PhaseGoalTransition::Continue(next_goal));
					}
				}

				Err(error)
			},
		}
	}

	fn phase_goal_spec(&self, phase: PhaseGoalKind, detail: Option<&str>) -> PhaseGoalSpec {
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
				detail.unwrap_or(
					"Run the registered canonicalize and verify commands before completing this phase."
				)
			),
			PhaseGoalKind::RepairAcceptedReviewFindings => format!(
				"Decodex phase: {}\nRepair accepted review findings for {} on the retained PR head without widening issue scope, including any required docs impact update or drift evidence. Do not request GitHub Review before Decodex validation. {phase_exit_contract}",
				phase.as_str(),
				self.issue_run.issue.identifier
			),
			PhaseGoalKind::ReviewRepairEvidence => format!(
				"Decodex phase: {}\nAfter Decodex validation, finish retained PR repair evidence for {}: record a current-HEAD `issue_progress_checkpoint` with `docs_impact`, push the current repaired branch to the retained PR branch, re-read the PR remote head and mergeability, record the required review-repair evidence, call `issue_review_repair_complete` for the same retained PR and pushed head, then call `issue_terminal_finalize` with path `review_repair`. Do not call `issue_review_handoff`, move the issue out of its retained review state, merge, or land the PR. Goal completion alone is not issue success.{}",
				phase.as_str(),
				self.issue_run.issue.identifier,
				detail.map_or_else(String::new, |detail| format!(" {detail}"))
			),
			PhaseGoalKind::HandoffEvidence => format!(
				"Decodex phase: {}\nAfter Decodex validation, prepare PR-backed handoff evidence for {}: record a current-HEAD `issue_progress_checkpoint` with `docs_impact`, run the bounded review policy as instructed, push the branch when ready, create or update the non-draft PR, then record the required Decodex terminal path. Goal completion alone is not issue success.{}",
				phase.as_str(),
				self.issue_run.issue.identifier,
				detail.map_or_else(String::new, |detail| format!(" {detail}"))
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
		repo_gate_outcome: &RepoGateCommandOutcome,
	) -> Result<PhaseAcceptanceCheck> {
		let fingerprint = loop_guardrail_worktree_fingerprint(&self.issue_run.worktree.path)?;
		let head_sha = fingerprint.as_ref().map(|value| value.head_sha.clone());
		let changed_surfaces = phase_acceptance_changed_surfaces(&self.issue_run.worktree.path);
		let effective_delta_present =
			fingerprint.as_ref().is_some_and(|value| value.effective_delta_present)
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
		let non_goal_violation =
			checkpoint_payload.is_some_and(phase_acceptance_has_non_goal_violation);
		let objective_covered = checkpoint.is_some()
			&& checkpoint_matches_head
			&& docs_impact_valid
			&& blocker_count == 0;
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
			repo_gate_tracked_rewrites: repo_gate_outcome.tracked_rewrite_decision().cloned(),
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

		Ok(events.into_iter().rev().find(|event| event.event_type() == "progress_checkpoint"))
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
					"tracked_rewrites": check
						.repo_gate_tracked_rewrites
						.as_ref()
						.map(RepoGateTrackedRewriteDecision::to_json),
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
			PhaseGoalKind::ReviewRepairEvidence | PhaseGoalKind::HandoffEvidence => {
				self.record_phase_goal_transition(
					phase,
					phase_terminal_goal_complete_signal(phase),
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
pub(super) struct PhaseAcceptanceCheckFailure {
	reason_code: String,
}
impl PhaseAcceptanceCheckFailure {
	fn new(reason_code: impl Into<String>) -> Self {
		Self { reason_code: reason_code.into() }
	}

	pub(super) fn error_class(&self) -> &'static str {
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
	repo_gate_tracked_rewrites: Option<RepoGateTrackedRewriteDecision>,
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
		| PhaseGoalKind::ReviewRepairEvidence
		| PhaseGoalKind::HandoffEvidence => PhaseGoalKind::RepairValidationFailures,
	}
}

fn phase_validation_pass_next_phase(phase: PhaseGoalKind) -> PhaseGoalKind {
	match phase {
		PhaseGoalKind::RepairAcceptedReviewFindings => PhaseGoalKind::ReviewRepairEvidence,
		PhaseGoalKind::ImplementToValidationReady | PhaseGoalKind::RepairValidationFailures => {
			PhaseGoalKind::HandoffEvidence
		},
		PhaseGoalKind::ReviewRepairEvidence | PhaseGoalKind::HandoffEvidence => phase,
	}
}

fn phase_terminal_goal_complete_signal(phase: PhaseGoalKind) -> &'static str {
	match phase {
		PhaseGoalKind::ReviewRepairEvidence => "review_repair_evidence_goal_complete",
		PhaseGoalKind::HandoffEvidence => "handoff_evidence_goal_complete",
		PhaseGoalKind::ImplementToValidationReady
		| PhaseGoalKind::RepairValidationFailures
		| PhaseGoalKind::RepairAcceptedReviewFindings => "phase_goal_complete",
	}
}

fn phase_tracked_rewrite_handoff_detail(
	next_phase: PhaseGoalKind,
	decision: &RepoGateTrackedRewriteDecision,
) -> String {
	let terminal_context = match next_phase {
		PhaseGoalKind::ReviewRepairEvidence => "review repair completion",
		_ => "review handoff",
	};

	format!(
		"Repo gate validation passed after rewriting owned tracked files: {}. Commit these issue-owned gate rewrites with the lane changes before {terminal_context}.",
		decision.files_display()
	)
}

fn phase_acceptance_changed_surfaces(worktree_path: &Path) -> Vec<String> {
	let mut surfaces = BTreeSet::new();

	if let Ok(changed_files) = repo_gate_changed_tracked_files(worktree_path) {
		surfaces.extend(changed_files);
	}
	if let Ok(Some(diff_paths)) = git_guardrail_output(
		worktree_path,
		&["diff", "--name-only", "--diff-filter=ACDMRTUXB", "HEAD", "--"],
	) {
		for path in diff_paths.lines().map(str::trim).filter(|path| !path.is_empty()) {
			surfaces.insert(path.to_owned());
		}
	}
	if let Ok(Some(status)) = git_guardrail_output(worktree_path, &["status", "--porcelain"]) {
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
	payload.get("blockers").and_then(Value::as_array).map_or(0, Vec::len)
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

pub(super) fn build_phase_goal_controller<'a>(
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	issue_run: &'a IssueRunPlan,
) -> RepoGatePhaseGoalController<'a> {
	RepoGatePhaseGoalController { project, workflow, state_store, issue_run }
}

pub(super) fn maybe_continue_after_phase_goal_recovery(
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
	)?
	else {
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

pub(super) fn recover_phase_goal_continuation(
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

	let Some(source_phase) = latest_phase_goal_recovery_candidate(project, state_store, issue_run)?
	else {
		return Ok(None);
	};
	let controller = RepoGatePhaseGoalController { project, workflow, state_store, issue_run };
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
			"progress_checkpoint" if progress_checkpoint_has_blockers(event.payload()) => {
				return Ok(None);
			},
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

pub(super) fn latest_open_issue_phase_goal_before_attempt(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_id: &str,
	current_run_id: &str,
	current_attempt_number: i64,
) -> Result<Option<PhaseGoalKind>> {
	if current_attempt_number <= 1 {
		return Ok(None);
	}

	let events =
		state_store.list_private_execution_events_for_issue(project.service_id(), issue_id)?;

	for event in events.iter().rev().filter(|event| {
		event.attempt_number() < current_attempt_number && event.run_id() != current_run_id
	}) {
		match event.event_type() {
			"terminal_finalize"
			| "review_completion_intent"
			| AUTHORITY_DECISION_REQUEST_EVENT_TYPE
			| PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE
			| RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE => return Ok(None),
			"progress_checkpoint" if progress_checkpoint_has_blockers(event.payload()) => {
				return Ok(None);
			},
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
	let events = state_store
		.list_private_execution_events_for_issue(project.service_id(), &issue_run.issue.id)?;

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

fn phase_goal_continuation_next_phase(event_type: &str, payload: &Value) -> Option<PhaseGoalKind> {
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
		"review_repair_evidence" => Some(PhaseGoalKind::ReviewRepairEvidence),
		"handoff_evidence" => Some(PhaseGoalKind::HandoffEvidence),
		_ => None,
	}
}
