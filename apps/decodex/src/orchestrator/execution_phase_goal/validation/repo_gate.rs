use crate::orchestrator::{
	self, LaneDecisionSnapshot, LoopGuardrailRecoveryDecision, PhaseGoalKind, PhaseGoalTransition,
	RepoGateCommandOutcome, RepoGateFailure, RepoGateFailureDisposition,
	RepoGateTrackedRewriteDecision, Report, ResolvedRepoGate, Result,
	execution_phase_goal::{
		acceptance::{self, PhaseAcceptanceDecision},
		controller::RepoGatePhaseGoalController,
	},
};

impl RepoGatePhaseGoalController<'_> {
	pub(in crate::orchestrator::execution_phase_goal) fn continue_after_repo_gate_pass(
		&self,
		phase: PhaseGoalKind,
		selected_repo_gate: &ResolvedRepoGate<'_>,
		repo_gate_outcome: &RepoGateCommandOutcome,
	) -> Result<PhaseGoalTransition> {
		let acceptance_check =
			self.evaluate_phase_acceptance(phase, selected_repo_gate, repo_gate_outcome)?;

		self.record_phase_acceptance_check(&acceptance_check)?;

		if acceptance_check.decision == PhaseAcceptanceDecision::Fail {
			return self.continue_after_phase_acceptance_failure(phase, &acceptance_check);
		}

		self.state_store.clear_loop_guardrail_checkpoints_for_issue(
			self.project.service_id(),
			&self.issue_run.issue.id,
		)?;

		let next_phase = acceptance::phase_validation_pass_next_phase(phase);
		let mut transition_payload = orchestrator::json!({ "nextPhase": next_phase.as_str() });

		if let Some(decision) = repo_gate_outcome.tracked_rewrite_decision() {
			transition_payload["trackedRewrites"] = decision.to_json();
		}

		self.record_phase_goal_transition(phase, "validation_pass", transition_payload)?;

		let handoff_detail = repo_gate_outcome
			.tracked_rewrite_decision()
			.map(|decision| acceptance::phase_tracked_rewrite_handoff_detail(next_phase, decision));
		let next_goal = self.phase_goal_spec(next_phase, handoff_detail.as_deref());

		self.persist_next_phase_goal(&next_goal, "validation_pass")?;

		Ok(PhaseGoalTransition::Continue(next_goal))
	}

	pub(in crate::orchestrator::execution_phase_goal) fn continue_after_repo_gate_error(
		&self,
		phase: PhaseGoalKind,
		error: Report,
	) -> Result<PhaseGoalTransition> {
		let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() else {
			return Err(error);
		};
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
		let lane_decision = orchestrator::decide_lane_next_action(&lane_snapshot);
		let mut transition_payload = orchestrator::json!({
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

		self.record_phase_goal_transition(phase, "validation_fail", transition_payload)?;
		self.record_lane_decision_snapshot(
			&lane_snapshot,
			lane_decision.next_action,
			lane_decision.reason,
		)?;

		if lane_decision.blocks_automatic_execution() {
			return Err(error);
		}

		let repair_target_detail = (repo_gate_failure.disposition()
			== RepoGateFailureDisposition::ContinueRepair
			&& lane_decision.permits_phase_repair_retry())
		.then(|| repo_gate_failure.repair_target_detail().to_owned());

		if let Some(repair_target_detail) = repair_target_detail {
			return self.continue_after_repairable_repo_gate_error(repair_target_detail, error);
		}

		Err(error)
	}

	fn continue_after_repairable_repo_gate_error(
		&self,
		repair_target_detail: String,
		error: Report,
	) -> Result<PhaseGoalTransition> {
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

		let detail = format!(
			"{} Inspect the worktree, run the registered canonicalize and verify commands, and repair only the validation failure.",
			repair_target_detail
		);
		let next_goal =
			self.phase_goal_spec(PhaseGoalKind::RepairValidationFailures, Some(&detail));

		self.persist_next_phase_goal(&next_goal, "validation_fail")?;

		Ok(PhaseGoalTransition::Continue(next_goal))
	}
}
