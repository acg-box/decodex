use serde_json::Value;

use crate::orchestrator::lane_decision::{json_projection, projection};
use crate::orchestrator::{
	IssueDispatchMode, PhaseGoalKind, RepoGateFailureDisposition, RetryKind,
	kernel::{
		action::OwnedLaneAction,
		command::CommandIntentKind,
		decision::OwnedLaneDecision,
		facts::LaneObservation,
		state::{LivenessState, TerminalizationState},
	},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(in crate::orchestrator) enum LaneNextAction {
	ContinueCurrentPhase,
	ResumeContinuation,
	RetryFailure,
	RunRepoGate,
	EnterReviewHandoff,
	WaitExternal,
	NeedsAttention,
	CleanupTerminal,
	ForbiddenStaleOrAmbiguous,
}
impl LaneNextAction {
	pub(in crate::orchestrator) const fn as_str(self) -> &'static str {
		match self {
			Self::ContinueCurrentPhase => "continue_current_phase",
			Self::ResumeContinuation => "resume_continuation",
			Self::RetryFailure => "retry_failure",
			Self::RunRepoGate => "run_repo_gate",
			Self::EnterReviewHandoff => "enter_review_handoff",
			Self::WaitExternal => "wait_external",
			Self::NeedsAttention => "needs_attention",
			Self::CleanupTerminal => "cleanup_terminal",
			Self::ForbiddenStaleOrAmbiguous => "forbidden_stale_or_ambiguous",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct LaneDecisionSnapshot {
	pub(in crate::orchestrator) issue_identifier: String,
	pub(in crate::orchestrator) run_id: String,
	pub(in crate::orchestrator) attempt_number: i64,
	pub(in crate::orchestrator) dispatch_mode: IssueDispatchMode,
	pub(in crate::orchestrator) active_phase: Option<PhaseGoalKind>,
	pub(in crate::orchestrator) continuation_pending: bool,
	pub(in crate::orchestrator) retry_kind: Option<RetryKind>,
	pub(in crate::orchestrator) retry_budget_consumed: bool,
	pub(in crate::orchestrator) progress_blocker_count: usize,
	pub(in crate::orchestrator) non_goal_violation: bool,
	pub(in crate::orchestrator) phase_acceptance_failure: bool,
	pub(in crate::orchestrator) repo_gate_disposition: Option<RepoGateFailureDisposition>,
	pub(in crate::orchestrator) scope_envelope_violation: bool,
	pub(in crate::orchestrator) ambiguous_lineage: bool,
	pub(in crate::orchestrator) terminal_evidence_present: bool,
}
impl LaneDecisionSnapshot {
	#[allow(clippy::too_many_arguments)]
	pub(in crate::orchestrator) fn phase_acceptance(
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

	#[allow(clippy::too_many_arguments)]
	pub(in crate::orchestrator) fn child_exit_retry(
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

	pub(in crate::orchestrator) fn repo_gate_failure(
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

	pub(in crate::orchestrator) fn to_json(&self, action: LaneNextAction, reason: &str) -> Value {
		let decision = projection::decide_lane_next_action(self);

		serde_json::json!({
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
			"kernel_decision": json_projection::owned_lane_decision_to_json(&decision.kernel_decision),
		})
	}

	pub(super) fn to_kernel_observation(&self) -> LaneObservation {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct LaneDecision {
	pub(in crate::orchestrator) next_action: LaneNextAction,
	pub(in crate::orchestrator) reason: &'static str,
	pub(in crate::orchestrator) kernel_decision: OwnedLaneDecision,
}
impl LaneDecision {
	pub(super) fn new(
		next_action: LaneNextAction,
		reason: &'static str,
		kernel_decision: OwnedLaneDecision,
	) -> Self {
		Self { next_action, reason, kernel_decision }
	}

	pub(in crate::orchestrator) fn permits_child_exit_retry_kind(&self, kind: RetryKind) -> bool {
		let required_intent = match kind {
			RetryKind::Continuation => CommandIntentKind::ResumeRetainedLane,
			RetryKind::Failure => CommandIntentKind::ScheduleRetry,
		};

		self.has_command_intent(required_intent)
	}

	pub(in crate::orchestrator) fn permits_phase_repair_retry(&self) -> bool {
		self.has_command_intent(CommandIntentKind::ScheduleRetry)
	}

	pub(in crate::orchestrator) fn blocks_automatic_execution(&self) -> bool {
		self.kernel_decision.decision_class == OwnedLaneAction::ManualInterventionRequired
	}

	fn has_command_intent(&self, kind: CommandIntentKind) -> bool {
		self.kernel_decision.command_intents.iter().any(|intent| intent.kind == kind)
	}
}
