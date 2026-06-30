//! Central lane next-action decisions shared by lifecycle adapters.

use super::{IssueDispatchMode, PhaseGoalKind, RepoGateFailureDisposition, RetryKind, json};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(super) enum LaneNextAction {
	ContinueCurrentPhase,
	ResumeContinuation,
	RetryFailure,
	RunRepoGate,
	EnterReviewHandoff,
	WaitExternal,
	NeedsAttention,
	StopBlocked,
	CleanupTerminal,
	ForbiddenStaleOrAmbiguous,
}
impl LaneNextAction {
	pub(super) const fn as_str(self) -> &'static str {
		match self {
			Self::ContinueCurrentPhase => "continue_current_phase",
			Self::ResumeContinuation => "resume_continuation",
			Self::RetryFailure => "retry_failure",
			Self::RunRepoGate => "run_repo_gate",
			Self::EnterReviewHandoff => "enter_review_handoff",
			Self::WaitExternal => "wait_external",
			Self::NeedsAttention => "needs_attention",
			Self::StopBlocked => "stop_blocked",
			Self::CleanupTerminal => "cleanup_terminal",
			Self::ForbiddenStaleOrAmbiguous => "forbidden_stale_or_ambiguous",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LaneDecisionSnapshot {
	pub(super) issue_identifier: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) dispatch_mode: IssueDispatchMode,
	pub(super) active_phase: Option<PhaseGoalKind>,
	pub(super) continuation_pending: bool,
	pub(super) retry_kind: Option<RetryKind>,
	pub(super) retry_budget_consumed: bool,
	pub(super) progress_blocker_count: usize,
	pub(super) non_goal_violation: bool,
	pub(super) phase_acceptance_failure: bool,
	pub(super) repo_gate_disposition: Option<RepoGateFailureDisposition>,
	pub(super) scope_envelope_violation: bool,
	pub(super) ambiguous_lineage: bool,
	pub(super) terminal_evidence_present: bool,
}
impl LaneDecisionSnapshot {
	pub(super) fn phase_acceptance(
		issue_identifier: impl Into<String>,
		run_id: impl Into<String>,
		attempt_number: i64,
		dispatch_mode: IssueDispatchMode,
		active_phase: PhaseGoalKind,
		progress_blocker_count: usize,
		non_goal_violation: bool,
		scope_envelope_violation: bool,
	) -> Self {
		Self {
			issue_identifier: issue_identifier.into(),
			run_id: run_id.into(),
			attempt_number,
			dispatch_mode,
			active_phase: Some(active_phase),
			continuation_pending: false,
			retry_kind: None,
			retry_budget_consumed: false,
			progress_blocker_count,
			non_goal_violation,
			phase_acceptance_failure: true,
			repo_gate_disposition: None,
			scope_envelope_violation,
			ambiguous_lineage: false,
			terminal_evidence_present: false,
		}
	}

	pub(super) fn child_exit_retry(
		issue_identifier: impl Into<String>,
		run_id: impl Into<String>,
		attempt_number: i64,
		dispatch_mode: IssueDispatchMode,
		continuation_pending: bool,
		retry_kind: Option<RetryKind>,
		progress_blocker_count: usize,
		non_goal_violation: bool,
		terminal_evidence_present: bool,
	) -> Self {
		Self {
			issue_identifier: issue_identifier.into(),
			run_id: run_id.into(),
			attempt_number,
			dispatch_mode,
			active_phase: None,
			continuation_pending,
			retry_kind,
			retry_budget_consumed: retry_kind.is_some_and(|kind| kind != RetryKind::Continuation),
			progress_blocker_count,
			non_goal_violation,
			phase_acceptance_failure: false,
			repo_gate_disposition: None,
			scope_envelope_violation: false,
			ambiguous_lineage: false,
			terminal_evidence_present,
		}
	}

	pub(super) fn repo_gate_failure(
		issue_identifier: impl Into<String>,
		run_id: impl Into<String>,
		attempt_number: i64,
		dispatch_mode: IssueDispatchMode,
		active_phase: PhaseGoalKind,
		repo_gate_disposition: RepoGateFailureDisposition,
		scope_envelope_violation: bool,
	) -> Self {
		Self {
			issue_identifier: issue_identifier.into(),
			run_id: run_id.into(),
			attempt_number,
			dispatch_mode,
			active_phase: Some(active_phase),
			continuation_pending: false,
			retry_kind: None,
			retry_budget_consumed: false,
			progress_blocker_count: 0,
			non_goal_violation: false,
			phase_acceptance_failure: false,
			repo_gate_disposition: Some(repo_gate_disposition),
			scope_envelope_violation,
			ambiguous_lineage: false,
			terminal_evidence_present: false,
		}
	}

	pub(super) fn to_json(&self, action: LaneNextAction, reason: &str) -> Value {
		json!({
			"schema": "decodex.lane_decision_snapshot/1",
			"issue_identifier": self.issue_identifier,
			"run_id": self.run_id,
			"attempt_number": self.attempt_number,
			"dispatch_mode": self.dispatch_mode.as_str(),
			"active_phase": self.active_phase.map(PhaseGoalKind::as_str),
			"continuation_pending": self.continuation_pending,
			"retry_kind": self.retry_kind.map(RetryKind::as_str),
			"retry_budget_consumed": self.retry_budget_consumed,
			"progress_blocker_count": self.progress_blocker_count,
			"non_goal_violation": self.non_goal_violation,
			"phase_acceptance_failure": self.phase_acceptance_failure,
			"repo_gate_disposition": self.repo_gate_disposition.map(RepoGateFailureDisposition::as_str),
			"scope_envelope_violation": self.scope_envelope_violation,
			"ambiguous_lineage": self.ambiguous_lineage,
			"terminal_evidence_present": self.terminal_evidence_present,
			"next_action": action.as_str(),
			"reason": reason,
		})
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LaneDecision {
	pub(super) next_action: LaneNextAction,
	pub(super) reason: &'static str,
}
impl LaneDecision {
	pub(super) const fn new(next_action: LaneNextAction, reason: &'static str) -> Self {
		Self { next_action, reason }
	}
}

pub(super) fn decide_lane_next_action(snapshot: &LaneDecisionSnapshot) -> LaneDecision {
	if snapshot.ambiguous_lineage {
		return LaneDecision::new(
			LaneNextAction::ForbiddenStaleOrAmbiguous,
			"lineage or ownership is ambiguous",
		);
	}
	if snapshot.terminal_evidence_present {
		return LaneDecision::new(LaneNextAction::CleanupTerminal, "terminal evidence is present");
	}
	if snapshot.progress_blocker_count > 0 || snapshot.non_goal_violation {
		return LaneDecision::new(
			LaneNextAction::NeedsAttention,
			"progress checkpoint carries blockers or non-goal violation",
		);
	}
	if snapshot.scope_envelope_violation {
		return LaneDecision::new(
			LaneNextAction::NeedsAttention,
			"repo-gate write-set crossed the lane scope envelope",
		);
	}
	if snapshot.phase_acceptance_failure {
		return LaneDecision::new(
			LaneNextAction::RetryFailure,
			"phase acceptance failure remains an issue-local repair",
		);
	}
	if let Some(disposition) = snapshot.repo_gate_disposition {
		return match disposition {
			RepoGateFailureDisposition::ContinueRepair => LaneDecision::new(
				LaneNextAction::RetryFailure,
				"repo-gate failure remains an issue-local repair",
			),
			RepoGateFailureDisposition::RetryAfterBackoff => LaneDecision::new(
				LaneNextAction::WaitExternal,
				"repo-gate failure requires backoff before retry",
			),
			RepoGateFailureDisposition::NeedsHumanAttention => LaneDecision::new(
				LaneNextAction::NeedsAttention,
				"repo-gate failure crossed an authority boundary",
			),
		};
	}
	if snapshot.continuation_pending {
		return LaneDecision::new(
			LaneNextAction::ResumeContinuation,
			"open phase continuation remains valid",
		);
	}
	if snapshot.retry_kind.is_some() {
		return LaneDecision::new(
			LaneNextAction::RetryFailure,
			"retryable failure remains in budget",
		);
	}
	if snapshot.active_phase.is_some() {
		return LaneDecision::new(
			LaneNextAction::RunRepoGate,
			"active phase is ready for repo gate",
		);
	}

	LaneDecision::new(LaneNextAction::ContinueCurrentPhase, "ordinary lane execution may continue")
}

pub(super) fn lane_decision_blocks_automatic_execution(action: LaneNextAction) -> bool {
	matches!(
		action,
		LaneNextAction::NeedsAttention
			| LaneNextAction::StopBlocked
			| LaneNextAction::ForbiddenStaleOrAmbiguous
	)
}
