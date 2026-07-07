use crate::{agent::PhaseGoalKind, orchestrator::RepoGateTrackedRewriteDecision};

pub(crate) fn phase_acceptance_reason_code(
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

pub(crate) fn phase_acceptance_repair_phase(phase: PhaseGoalKind) -> PhaseGoalKind {
	match phase {
		PhaseGoalKind::RepairAcceptedReviewFindings => PhaseGoalKind::RepairAcceptedReviewFindings,
		PhaseGoalKind::ImplementToValidationReady
		| PhaseGoalKind::RepairValidationFailures
		| PhaseGoalKind::ReviewRepairEvidence
		| PhaseGoalKind::HandoffEvidence => PhaseGoalKind::RepairValidationFailures,
	}
}

pub(crate) fn phase_validation_pass_next_phase(phase: PhaseGoalKind) -> PhaseGoalKind {
	match phase {
		PhaseGoalKind::RepairAcceptedReviewFindings => PhaseGoalKind::ReviewRepairEvidence,
		PhaseGoalKind::ImplementToValidationReady | PhaseGoalKind::RepairValidationFailures => {
			PhaseGoalKind::HandoffEvidence
		},
		PhaseGoalKind::ReviewRepairEvidence | PhaseGoalKind::HandoffEvidence => phase,
	}
}

pub(crate) fn phase_terminal_goal_complete_signal(phase: PhaseGoalKind) -> &'static str {
	match phase {
		PhaseGoalKind::ReviewRepairEvidence => "review_repair_evidence_goal_complete",
		PhaseGoalKind::HandoffEvidence => "handoff_evidence_goal_complete",
		PhaseGoalKind::ImplementToValidationReady
		| PhaseGoalKind::RepairValidationFailures
		| PhaseGoalKind::RepairAcceptedReviewFindings => "phase_goal_complete",
	}
}

pub(crate) fn phase_tracked_rewrite_handoff_detail(
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
