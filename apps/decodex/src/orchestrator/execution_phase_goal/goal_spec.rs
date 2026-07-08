use crate::orchestrator::{
	PhaseGoalKind, PhaseGoalSpec, execution_phase_goal::controller::RepoGatePhaseGoalController,
};

impl RepoGatePhaseGoalController<'_> {
	pub(super) fn phase_goal_spec(
		&self,
		phase: PhaseGoalKind,
		detail: Option<&str>,
	) -> PhaseGoalSpec {
		let phase_exit_contract = "When the phase objective is satisfied, mark the active phase goal complete. Decodex owns repository validation and records structured validation evidence before selecting the next lifecycle step.";
		let objective = match phase {
			PhaseGoalKind::ImplementToValidationReady => format!(
				"Decodex step: implementation\nProduce the smallest coherent implementation and documentation change for {} that is ready for the registered Decodex repo gate. Do not push, request review, or treat goal completion as issue completion. {phase_exit_contract}",
				self.issue_run.issue.identifier
			),
			PhaseGoalKind::RepairValidationFailures => format!(
				"Decodex step: validation repair\nRepair repo-gate failures for {} in the current worktree without widening issue scope. {} {phase_exit_contract}",
				self.issue_run.issue.identifier,
				detail.unwrap_or(
					"Run the registered canonicalize and verify commands before completing this phase."
				)
			),
			PhaseGoalKind::RepairAcceptedReviewFindings => format!(
				"Decodex step: review repair\nRepair accepted review findings for {} on the retained PR head without widening issue scope. Do not request GitHub Review before Decodex validation. {phase_exit_contract}",
				self.issue_run.issue.identifier
			),
			PhaseGoalKind::ReviewRepairEvidence => format!(
				"Decodex step: review repair evidence\nAfter Decodex validation for {}, push the current repaired branch to the retained PR branch, re-read the PR remote head and mergeability, record the required review-repair evidence, call `issue_review_repair_complete` for the same retained PR and pushed head, then call `issue_terminal_finalize` with path `review_repair`. Do not call `issue_review_handoff`, move the issue out of its retained review state, merge, or land the PR.{}",
				self.issue_run.issue.identifier,
				detail.map_or_else(String::new, |detail| format!(" {detail}"))
			),
			PhaseGoalKind::HandoffEvidence => format!(
				"Decodex step: review handoff\nAfter Decodex validation, prepare PR-backed handoff evidence for {}: run the bounded review policy as instructed, push the branch when ready, create or update the non-draft PR, then record the required Decodex terminal path.{}",
				self.issue_run.issue.identifier,
				detail.map_or_else(String::new, |detail| format!(" {detail}"))
			),
		};

		PhaseGoalSpec::new(phase, objective, None)
	}
}
