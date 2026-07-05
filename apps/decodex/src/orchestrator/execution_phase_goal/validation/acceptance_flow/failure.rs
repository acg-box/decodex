use crate::orchestrator::{
	self, LaneDecisionSnapshot, LoopGuardrailRecoveryDecision, ManualAttentionRequested,
	PhaseGoalKind, PhaseGoalTransition, RepoGateFailureDisposition, Report, Result,
	execution_phase_goal::{
		acceptance::{self, PhaseAcceptanceCheck, PhaseAcceptanceCheckFailure},
		controller::RepoGatePhaseGoalController,
	},
};

impl RepoGatePhaseGoalController<'_> {
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
			return self.manual_attention_requested(error_class, error);
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
			return self.manual_attention_requested(error_class, error);
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

	fn manual_attention_requested(
		&self,
		error_class: &str,
		error: Report,
	) -> Result<PhaseGoalTransition> {
		Err(Report::new(ManualAttentionRequested {
			issue_identifier: self.issue_run.issue.identifier.clone(),
			label: self.workflow.frontmatter().tracker().needs_attention_label().to_owned(),
			run_id: self.issue_run.run_id.clone(),
			error_class: Some(error_class.to_owned()),
		})
		.wrap_err(error))
	}
}
