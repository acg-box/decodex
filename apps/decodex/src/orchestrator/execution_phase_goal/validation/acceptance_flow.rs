use serde_json::Value;

use crate::{
	orchestrator::{
		self, LaneDecisionSnapshot, LoopGuardrailRecoveryDecision, ManualAttentionRequested,
		PhaseGoalKind, PhaseGoalTransition, RepoGateCommandOutcome, RepoGateFailureDisposition,
		Report, ResolvedRepoGate, Result,
		execution_phase_goal::{
			acceptance::{
				self, PhaseAcceptanceCheck, PhaseAcceptanceCheckFailure, PhaseAcceptanceDecision,
				phase_acceptance_blocker_count, phase_acceptance_docs_impact_valid,
				phase_acceptance_has_non_goal_violation,
			},
			controller::RepoGatePhaseGoalController,
		},
	},
	state,
};

impl RepoGatePhaseGoalController<'_> {
	pub(in crate::orchestrator::execution_phase_goal) fn evaluate_phase_acceptance(
		&self,
		phase: PhaseGoalKind,
		repo_gate: &ResolvedRepoGate<'_>,
		repo_gate_outcome: &RepoGateCommandOutcome,
	) -> Result<PhaseAcceptanceCheck> {
		let fingerprint =
			orchestrator::loop_guardrail_worktree_fingerprint(&self.issue_run.worktree.path)?;
		let head_sha = fingerprint.as_ref().map(|value| value.head_sha.clone());
		let changed_surfaces =
			acceptance::phase_acceptance_changed_surfaces(&self.issue_run.worktree.path);
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
		let reason_code = acceptance::phase_acceptance_reason_code(
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

	pub(in crate::orchestrator::execution_phase_goal) fn continue_after_phase_acceptance_failure(
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
		let lane_decision = orchestrator::decide_lane_next_action(&lane_snapshot);

		self.record_phase_goal_transition(
			phase,
			"validation_fail",
			orchestrator::json!({
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

		if lane_decision.blocks_automatic_execution() {
			return Err(Report::new(ManualAttentionRequested {
				issue_identifier: self.issue_run.issue.identifier.clone(),
				label: self.workflow.frontmatter().tracker().needs_attention_label().to_owned(),
				run_id: self.issue_run.run_id.clone(),
				error_class: Some(error_class.to_owned()),
			})
			.wrap_err(error));
		}

		if let Some(loop_guardrail_stop) = orchestrator::retryable_failure_loop_guardrail_stop(
			self.project,
			self.state_store,
			self.issue_run,
			&error,
		)? {
			match orchestrator::loop_guardrail_architecture_recovery_decision(
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

		if !lane_decision.permits_phase_repair_retry() {
			return Err(Report::new(ManualAttentionRequested {
				issue_identifier: self.issue_run.issue.identifier.clone(),
				label: self.workflow.frontmatter().tracker().needs_attention_label().to_owned(),
				run_id: self.issue_run.run_id.clone(),
				error_class: Some(error_class.to_owned()),
			})
			.wrap_err(error));
		}

		let next_phase = acceptance::phase_acceptance_repair_phase(phase);
		let detail = format!(
			"Phase acceptance check failed after repo gate pass with `{}`. {}",
			acceptance_check.reason_code,
			acceptance_check.next_action()
		);
		let next_goal = self.phase_goal_spec(next_phase, Some(&detail));

		self.persist_next_phase_goal(&next_goal, "phase_acceptance_fail")?;

		Ok(PhaseGoalTransition::Continue(next_goal))
	}
}
