use crate::orchestrator::execution_phase_goal::controller::RepoGatePhaseGoalController;
use crate::orchestrator::{PhaseGoalKind, PhaseGoalSpec};

impl RepoGatePhaseGoalController<'_> {
	pub(super) fn phase_goal_spec(
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
}
