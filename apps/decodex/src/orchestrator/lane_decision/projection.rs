use crate::orchestrator::{
	RepoGateFailureDisposition,
	kernel::{action::OwnedLaneAction, decision, decision::OwnedLaneDecision},
	lane_decision::model::{LaneDecision, LaneDecisionSnapshot, LaneNextAction},
};

pub(in crate::orchestrator) fn decide_lane_next_action(
	snapshot: &LaneDecisionSnapshot,
) -> LaneDecision {
	let kernel_decision = decision::decide_owned_lane(&snapshot.to_kernel_observation());
	let next_action = project_lane_next_action(snapshot, &kernel_decision);
	let reason = project_lane_reason(snapshot, &kernel_decision, next_action);

	LaneDecision::new(next_action, reason, kernel_decision)
}

fn project_lane_next_action(
	snapshot: &LaneDecisionSnapshot,
	decision: &OwnedLaneDecision,
) -> LaneNextAction {
	match decision.decision_class {
		OwnedLaneAction::ManualInterventionRequired =>
			if snapshot.ambiguous_lineage {
				LaneNextAction::ForbiddenStaleOrAmbiguous
			} else {
				LaneNextAction::NeedsAttention
			},
		OwnedLaneAction::Continue =>
			if snapshot.terminal_evidence_present {
				LaneNextAction::CleanupTerminal
			} else if snapshot.active_phase.is_some() && !snapshot.phase_acceptance_failure {
				LaneNextAction::RunRepoGate
			} else {
				LaneNextAction::ContinueCurrentPhase
			},
		OwnedLaneAction::RetryAutomatically => LaneNextAction::RetryFailure,
		OwnedLaneAction::ResumeRetainedLane => LaneNextAction::ResumeContinuation,
		OwnedLaneAction::WaitForExternalSignal => LaneNextAction::WaitExternal,
		OwnedLaneAction::ReadyToLand => LaneNextAction::EnterReviewHandoff,
	}
}

fn project_lane_reason(
	snapshot: &LaneDecisionSnapshot,
	decision: &OwnedLaneDecision,
	next_action: LaneNextAction,
) -> &'static str {
	if snapshot.ambiguous_lineage {
		return "lineage or ownership is ambiguous";
	}
	if snapshot.terminal_evidence_present {
		return "terminal evidence is present";
	}
	if snapshot.progress_blocker_count > 0 || snapshot.non_goal_violation {
		return "progress checkpoint carries blockers or non-goal violation";
	}
	if snapshot.scope_envelope_violation {
		return "repo-gate write-set crossed the lane scope envelope";
	}
	if snapshot.phase_acceptance_failure {
		return "phase acceptance failure remains an issue-local repair";
	}

	if let Some(disposition) = snapshot.repo_gate_disposition {
		return match disposition {
			RepoGateFailureDisposition::ContinueRepair =>
				"repo-gate failure remains an issue-local repair",
			RepoGateFailureDisposition::RetryAfterBackoff =>
				"repo-gate failure requires backoff before retry",
			RepoGateFailureDisposition::NeedsHumanAttention =>
				"repo-gate failure crossed an authority boundary",
		};
	}

	if snapshot.continuation_pending {
		return "open phase continuation remains valid";
	}
	if snapshot.retry_kind.is_some() {
		return "retryable failure remains in budget";
	}
	if snapshot.active_phase.is_some() {
		return "active phase is ready for repo gate";
	}

	match next_action {
		LaneNextAction::WaitExternal => "external signal remains pending",
		LaneNextAction::NeedsAttention | LaneNextAction::ForbiddenStaleOrAmbiguous => decision
			.blockers
			.first()
			.map_or("lane requires manual intervention", |blocker| blocker.public_summary),
		_ => "ordinary lane execution may continue",
	}
}
