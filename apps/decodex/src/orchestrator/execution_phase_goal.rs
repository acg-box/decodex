//! Phase-goal controller, acceptance checks, and continuation recovery.

mod acceptance;
mod recovery;

#[cfg(test)] pub(super) use self::recovery::latest_phase_goal_recovery_candidate;
pub(super) use self::{
	acceptance::PhaseAcceptanceCheckFailure,
	recovery::{
		PhaseGoalRecoveryContinuation, issue_has_blocking_lane_decision_evidence,
		latest_open_issue_phase_goal_before_attempt, maybe_continue_after_phase_goal_recovery,
		recover_phase_goal_continuation,
	},
};
use self::{
	acceptance::{
		PhaseAcceptanceCheck, PhaseAcceptanceDecision, phase_acceptance_blocker_count,
		phase_acceptance_changed_surfaces, phase_acceptance_docs_impact_valid,
		phase_acceptance_has_non_goal_violation, phase_acceptance_reason_code,
		phase_acceptance_repair_phase, phase_terminal_goal_complete_signal,
		phase_tracked_rewrite_handoff_detail, phase_validation_pass_next_phase,
	},
	recovery::phase_goal_kind_from_str,
};
use super::{
	IssueDispatchMode, IssueRunPlan, LaneDecisionSnapshot, LaneNextAction,
	LoopGuardrailRecoveryDecision, ManualAttentionRequested, PHASE_ACCEPTANCE_CHECK_EVENT_TYPE,
	PhaseGoalController, PhaseGoalKind, PhaseGoalSpec, PhaseGoalTransition,
	RUN_OPERATION_REPO_GATE, RepoGateCommandOutcome, RepoGateFailure, RepoGateFailureDisposition,
	RepoGateTrackedRewriteDecision, Report, ResolvedRepoGate, Result, ServiceConfig, StateStore,
	Value, WorkflowDocument, decide_lane_next_action, json,
	lane_decision_blocks_automatic_execution, loop_guardrail_architecture_recovery_decision,
	loop_guardrail_worktree_fingerprint, retryable_failure_loop_guardrail_stop,
	run_repo_gate_commands_allow_owned_tracked_rewrites, select_repo_gate_for_worktree, state,
	write_run_operation_marker_best_effort,
};

pub(super) struct RepoGatePhaseGoalController<'a> {
	pub(super) project: &'a ServiceConfig,
	pub(super) workflow: &'a WorkflowDocument,
	pub(super) state_store: &'a StateStore,
	pub(super) issue_run: &'a IssueRunPlan,
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
					let scope_envelope_violation = repo_gate_failure
						.tracked_rewrite_decision()
						.is_some_and(RepoGateTrackedRewriteDecision::is_scope_envelope_violation);
					let lane_snapshot = LaneDecisionSnapshot::repo_gate_failure(
						self.issue_run.issue.identifier.clone(),
						self.issue_run.run_id.clone(),
						self.issue_run.attempt_number,
						self.issue_run.dispatch_mode,
						phase,
						repo_gate_failure.disposition(),
						scope_envelope_violation,
					);
					let lane_decision = decide_lane_next_action(&lane_snapshot);
					let mut transition_payload = json!({
						"errorClass": repo_gate_failure.error_class(),
						"disposition": repo_gate_failure.disposition().as_str(),
						"laneDecision": lane_decision.next_action.as_str(),
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
					self.record_lane_decision_snapshot(
						&lane_snapshot,
						lane_decision.next_action,
						lane_decision.reason,
					)?;

					if lane_decision_blocks_automatic_execution(lane_decision.next_action) {
						return Err(error);
					}

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
		let lane_snapshot = LaneDecisionSnapshot::phase_acceptance(
			self.issue_run.issue.identifier.clone(),
			self.issue_run.run_id.clone(),
			self.issue_run.attempt_number,
			self.issue_run.dispatch_mode,
			phase,
			acceptance_check.blocker_count,
			!acceptance_check.non_goal_passed,
			false,
		);
		let lane_decision = decide_lane_next_action(&lane_snapshot);

		self.record_phase_goal_transition(
			phase,
			"validation_fail",
			json!({
				"errorClass": error_class,
				"disposition": RepoGateFailureDisposition::ContinueRepair.as_str(),
				"acceptanceDecision": acceptance_check.decision.as_str(),
				"acceptanceReason": acceptance_check.reason_code,
				"laneDecision": lane_decision.next_action.as_str(),
			}),
		)?;
		self.record_lane_decision_snapshot(
			&lane_snapshot,
			lane_decision.next_action,
			lane_decision.reason,
		)?;

		if lane_decision_blocks_automatic_execution(lane_decision.next_action) {
			return Err(Report::new(ManualAttentionRequested {
				issue_identifier: self.issue_run.issue.identifier.clone(),
				label: self.workflow.frontmatter().tracker().needs_attention_label().to_owned(),
				run_id: self.issue_run.run_id.clone(),
				error_class: Some(error_class.to_owned()),
			})
			.wrap_err(error));
		}

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

		match lane_decision.next_action {
			LaneNextAction::RetryFailure => {},
			LaneNextAction::ContinueCurrentPhase
			| LaneNextAction::ResumeContinuation
			| LaneNextAction::RunRepoGate
			| LaneNextAction::EnterReviewHandoff
			| LaneNextAction::WaitExternal
			| LaneNextAction::NeedsAttention
			| LaneNextAction::StopBlocked
			| LaneNextAction::CleanupTerminal
			| LaneNextAction::ForbiddenStaleOrAmbiguous => {
				return Err(Report::new(ManualAttentionRequested {
					issue_identifier: self.issue_run.issue.identifier.clone(),
					label: self.workflow.frontmatter().tracker().needs_attention_label().to_owned(),
					run_id: self.issue_run.run_id.clone(),
					error_class: Some(error_class.to_owned()),
				})
				.wrap_err(error));
			},
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

	fn record_lane_decision_snapshot(
		&self,
		snapshot: &LaneDecisionSnapshot,
		action: LaneNextAction,
		reason: &str,
	) -> Result<()> {
		self.state_store.append_private_execution_event(
			self.project.service_id(),
			&self.issue_run.issue.id,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
			"lane_decision",
			snapshot.to_json(action, reason),
		)?;

		Ok(())
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

pub(super) fn build_phase_goal_controller<'a>(
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	issue_run: &'a IssueRunPlan,
) -> RepoGatePhaseGoalController<'a> {
	RepoGatePhaseGoalController { project, workflow, state_store, issue_run }
}
