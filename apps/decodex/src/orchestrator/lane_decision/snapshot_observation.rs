use crate::orchestrator::{
	RepoGateFailureDisposition,
	kernel::{
		facts::LaneObservation,
		state::{LivenessState, TerminalizationState},
	},
	lane_decision::model::LaneDecisionSnapshot,
};

impl LaneDecisionSnapshot {
	pub(in crate::orchestrator) fn to_kernel_observation(&self) -> LaneObservation {
		let mut observation = LaneObservation::for_issue(self.issue_identifier.clone());

		observation.run_id = Some(self.run_id.clone());
		observation.authority_complete = true;
		observation.run_lease = true;
		observation.active_owned_work = true;
		observation.liveness = LivenessState::ThreadActive;
		observation.terminalization = if self.terminal_evidence_present {
			TerminalizationState::CleanupPending
		} else {
			TerminalizationState::None
		};
		observation.contradictory_authority = self.ambiguous_lineage;
		observation.human_attention_signal = self.progress_blocker_count > 0
			|| self.non_goal_violation
			|| self.scope_envelope_violation
			|| self.repo_gate_disposition == Some(RepoGateFailureDisposition::NeedsHumanAttention);
		observation.retry_budget_available = self.phase_acceptance_failure
			|| self.retry_kind.is_some()
			|| self.repo_gate_disposition == Some(RepoGateFailureDisposition::ContinueRepair);
		observation.retry_budget_exhausted = false;
		observation.retained_lane_reusable = self.continuation_pending;
		observation.external_signal_pending =
			self.repo_gate_disposition == Some(RepoGateFailureDisposition::RetryAfterBackoff);

		observation
	}
}
