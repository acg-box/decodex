use crate::orchestrator::{
	self, LaneDecisionSnapshot, LoopGuardrailRecoveryDecision, ManualAttentionRequested,
	PhaseGoalKind, PhaseGoalTransition, RepoGateFailureDisposition, Report, Result,
	execution_phase_goal::{
		acceptance::{self, ValidationEvidence, ValidationEvidenceFailure},
		controller::RepoGatePhaseGoalController,
	},
};
use crate::lane_authority::{
	LaneId, NoEffectiveDeltaCommand, NoEffectiveDeltaDecision,
};

impl RepoGatePhaseGoalController<'_> {
	pub(in crate::orchestrator::execution_phase_goal) fn continue_after_validation_evidence_failure(
		&self,
		phase: PhaseGoalKind,
		acceptance_check: &ValidationEvidence,
	) -> Result<PhaseGoalTransition> {
		let failure = ValidationEvidenceFailure::new(acceptance_check.reason_code);
		let error_class = failure.error_class();
		let error = Report::new(failure);
		if acceptance_check.reason_code == "no_effective_delta"
			&& let Some(transition) = self.continue_after_no_effective_delta(phase, acceptance_check)?
		{
			return Ok(transition);
		}
		let lane_snapshot = LaneDecisionSnapshot::validation_evidence(
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

		let next_phase = acceptance::validation_evidence_repair_phase(phase);
		let detail = format!(
			"Validation evidence failed after repo gate pass with `{}`. {}",
			acceptance_check.reason_code,
			acceptance_check.next_action()
		);
		let next_goal = self.phase_goal_spec(next_phase, Some(&detail));

		self.persist_next_phase_goal(&next_goal, "validation_evidence_fail")?;

		Ok(PhaseGoalTransition::Continue(next_goal))
	}

	fn continue_after_no_effective_delta(
		&self,
		phase: PhaseGoalKind,
		acceptance_check: &ValidationEvidence,
	) -> Result<Option<PhaseGoalTransition>> {
		let operation_id = acceptance_check
			.no_effective_delta_operation_id
			.as_ref()
			.ok_or_else(|| orchestrator::eyre::eyre!("No-effective-delta operation is missing."))?;
		let facts = acceptance_check
			.no_effective_delta_facts
			.clone()
			.ok_or_else(|| orchestrator::eyre::eyre!("No-effective-delta facts are missing."))?;
		let lane_id = LaneId::new(self.project.service_id(), &self.issue_run.issue.id)?;
		let current = self.state_store.no_effective_delta_recovery(operation_id)?;
		let command = if current.as_ref().is_some_and(|recovery| {
			self.issue_run.attempt_number > recovery.source_attempt_number()
		}) {
			NoEffectiveDeltaCommand::ObserveRetryResult {
				operation_id: operation_id.clone(),
				lane_id,
				attempt_number: self.issue_run.attempt_number,
				facts,
			}
		} else {
			NoEffectiveDeltaCommand::Observe {
				operation_id: operation_id.clone(),
				lane_id,
				attempt_number: self.issue_run.attempt_number,
				facts,
			}
		};
		match self.state_store.decide_no_effective_delta(operation_id, command)? {
			NoEffectiveDeltaDecision::Retry(recovery) => {
				let next_phase = acceptance::validation_evidence_repair_phase(phase);
				let detail = format!(
					"No effective delta was observed. Run the one bounded repair continuation with recovery `{}` and preserve its complete diagnostics.",
					recovery.idempotency_key(),
				);
				let next_goal = self.phase_goal_spec(next_phase, Some(&detail));
				self.record_phase_goal_transition(
					phase,
					"no_effective_delta_retry_scheduled",
					orchestrator::json!({
						"operationId": operation_id,
						"ordinal": recovery.ordinal(),
						"idempotencyKey": recovery.idempotency_key(),
					}),
				)?;
				self.persist_next_phase_goal(&next_goal, "no_effective_delta_retry_scheduled")?;
				Ok(Some(PhaseGoalTransition::ScheduleContinuation(next_goal)))
			},
			NoEffectiveDeltaDecision::AttentionRequired { reason_code, .. } => {
				self.record_phase_goal_transition(
					phase,
					"no_effective_delta_attention_required",
					orchestrator::json!({
						"operationId": operation_id,
						"reasonCode": reason_code,
					}),
				)?;
				self.manual_attention_requested(
					reason_code,
					Report::new(ValidationEvidenceFailure::new(reason_code)),
				)
				.map(Some)
			},
			NoEffectiveDeltaDecision::Blocked => Ok(None),
			NoEffectiveDeltaDecision::AlreadySatisfied { .. } => {
				orchestrator::eyre::bail!("Already-satisfied authority cannot originate from no-delta observation.")
			},
		}
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
