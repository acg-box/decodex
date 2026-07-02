//! Compatibility projection for lifecycle decisions shared by adapters.

use super::{
	IssueDispatchMode, PhaseGoalKind, RepoGateFailureDisposition, RetryKind, json,
	kernel::{
		action::OwnedLaneAction,
		command::{CommandFact, CommandIntent, CommandIntentKind},
		decision::{DecisionBlocker, OwnedLaneDecision, decide_owned_lane},
		facts::LaneObservation,
		state::{LaneStateAxes, LivenessState, TerminalizationState},
	},
};
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
	#[allow(clippy::too_many_arguments)]
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

	#[allow(clippy::too_many_arguments)]
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
		let decision = decide_lane_next_action(self);

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
			"kernel_decision": owned_lane_decision_to_json(&decision.kernel_decision),
		})
	}

	fn to_kernel_observation(&self) -> LaneObservation {
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
pub(super) struct LaneDecision {
	pub(super) next_action: LaneNextAction,
	pub(super) reason: &'static str,
	pub(super) kernel_decision: OwnedLaneDecision,
}
impl LaneDecision {
	fn new(
		next_action: LaneNextAction,
		reason: &'static str,
		kernel_decision: OwnedLaneDecision,
	) -> Self {
		Self { next_action, reason, kernel_decision }
	}

	pub(super) fn permits_child_exit_retry_kind(&self, kind: RetryKind) -> bool {
		let required_intent = match kind {
			RetryKind::Continuation => CommandIntentKind::ResumeRetainedLane,
			RetryKind::Failure => CommandIntentKind::ScheduleRetry,
		};

		self.has_command_intent(required_intent)
	}

	pub(super) fn permits_phase_repair_retry(&self) -> bool {
		self.has_command_intent(CommandIntentKind::ScheduleRetry)
	}

	pub(super) fn blocks_automatic_execution(&self) -> bool {
		self.kernel_decision.decision_class == OwnedLaneAction::ManualInterventionRequired
	}

	fn has_command_intent(&self, kind: CommandIntentKind) -> bool {
		self.kernel_decision.command_intents.iter().any(|intent| intent.kind == kind)
	}
}

pub(super) fn decide_lane_next_action(snapshot: &LaneDecisionSnapshot) -> LaneDecision {
	let kernel_decision = decide_owned_lane(&snapshot.to_kernel_observation());
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

fn owned_lane_decision_to_json(decision: &OwnedLaneDecision) -> Value {
	json!({
		"decision_class": decision.decision_class.as_str(),
		"policy_state": decision.policy_state.as_str(),
		"lane_state_axes": lane_state_axes_to_json(decision.lane_state_axes),
		"command_intents": decision
			.command_intents
			.iter()
			.map(command_intent_to_json)
			.collect::<Vec<_>>(),
		"projection_hints": {
			"lane_control_next_action": decision.projection_hints.lane_control_next_action,
			"primary_reason": decision.projection_hints.primary_reason.as_str(),
		},
		"blockers": decision
			.blockers
			.iter()
			.map(decision_blocker_to_json)
			.collect::<Vec<_>>(),
	})
}

fn lane_state_axes_to_json(axes: LaneStateAxes) -> Value {
	json!({
		"ownership": axes.ownership.as_str(),
		"liveness": axes.liveness.as_str(),
		"policy": axes.policy.as_str(),
		"terminalization": axes.terminalization.as_str(),
	})
}

fn command_intent_to_json(intent: &CommandIntent) -> Value {
	json!({
		"kind": intent.kind.as_str(),
		"idempotency_key": intent.idempotency_key,
		"preconditions": facts_to_json(&intent.preconditions),
		"expected_postconditions": facts_to_json(&intent.expected_postconditions),
	})
}

fn facts_to_json(facts: &[CommandFact]) -> Vec<&'static str> {
	facts.iter().map(|fact| fact.as_str()).collect()
}

fn decision_blocker_to_json(blocker: &DecisionBlocker) -> Value {
	json!({
		"reason": blocker.reason.as_str(),
		"public_summary": blocker.public_summary,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn repo_gate_snapshot(disposition: RepoGateFailureDisposition) -> LaneDecisionSnapshot {
		LaneDecisionSnapshot::repo_gate_failure(
			"PUB-101",
			"run-1",
			1,
			IssueDispatchMode::Normal,
			PhaseGoalKind::ImplementToValidationReady,
			disposition,
			false,
		)
	}

	#[test]
	fn phase_acceptance_failure_projects_kernel_retry_to_legacy_retry_failure() {
		let snapshot = LaneDecisionSnapshot::phase_acceptance(
			"PUB-101",
			"run-1",
			1,
			IssueDispatchMode::Normal,
			PhaseGoalKind::ImplementToValidationReady,
			0,
			false,
			false,
		);

		let decision = decide_lane_next_action(&snapshot);

		assert_eq!(decision.next_action, LaneNextAction::RetryFailure);
		assert_eq!(decision.kernel_decision.decision_class, OwnedLaneAction::RetryAutomatically);
		assert_eq!(decision.kernel_decision.command_intents[0].kind.as_str(), "schedule_retry");
		assert!(decision.permits_child_exit_retry_kind(RetryKind::Failure));
		assert!(!decision.permits_child_exit_retry_kind(RetryKind::Continuation));
		assert!(decision.permits_phase_repair_retry());
		assert!(!decision.blocks_automatic_execution());
		assert_eq!(
			snapshot.to_json(decision.next_action, decision.reason)["kernel_decision"]["decision_class"],
			"retry_automatically"
		);
	}

	#[test]
	fn repo_gate_backoff_projects_kernel_wait_to_legacy_wait_external() {
		let snapshot = repo_gate_snapshot(RepoGateFailureDisposition::RetryAfterBackoff);

		let decision = decide_lane_next_action(&snapshot);

		assert_eq!(decision.next_action, LaneNextAction::WaitExternal);
		assert_eq!(decision.kernel_decision.decision_class, OwnedLaneAction::WaitForExternalSignal);
		assert_eq!(decision.kernel_decision.command_intents[0].kind.as_str(), "wait_external");
		assert!(!decision.permits_phase_repair_retry());
	}

	#[test]
	fn repo_gate_continue_repair_requires_kernel_retry_intent() {
		let snapshot = repo_gate_snapshot(RepoGateFailureDisposition::ContinueRepair);

		let decision = decide_lane_next_action(&snapshot);

		assert_eq!(decision.next_action, LaneNextAction::RetryFailure);
		assert_eq!(decision.kernel_decision.decision_class, OwnedLaneAction::RetryAutomatically);
		assert!(decision.permits_phase_repair_retry());
	}

	#[test]
	fn scope_envelope_violation_projects_kernel_manual_to_legacy_attention() {
		let mut snapshot = repo_gate_snapshot(RepoGateFailureDisposition::NeedsHumanAttention);

		snapshot.scope_envelope_violation = true;

		let decision = decide_lane_next_action(&snapshot);

		assert_eq!(decision.next_action, LaneNextAction::NeedsAttention);
		assert_eq!(
			decision.kernel_decision.decision_class,
			OwnedLaneAction::ManualInterventionRequired
		);
		assert_eq!(
			decision.kernel_decision.blockers[0].public_summary,
			"human attention was requested for this lane"
		);
		assert!(decision.blocks_automatic_execution());
		assert!(!decision.permits_phase_repair_retry());
	}

	#[test]
	fn child_exit_continuation_projects_kernel_resume_to_legacy_resume() {
		let snapshot = LaneDecisionSnapshot::child_exit_retry(
			"PUB-101",
			"run-1",
			1,
			IssueDispatchMode::Retry,
			true,
			Some(RetryKind::Continuation),
			0,
			false,
			false,
		);

		let decision = decide_lane_next_action(&snapshot);

		assert_eq!(decision.next_action, LaneNextAction::ResumeContinuation);
		assert_eq!(decision.kernel_decision.decision_class, OwnedLaneAction::ResumeRetainedLane);
		assert_eq!(
			decision.kernel_decision.command_intents[0].kind.as_str(),
			"resume_retained_lane"
		);
		assert!(decision.permits_child_exit_retry_kind(RetryKind::Continuation));
		assert!(!decision.permits_child_exit_retry_kind(RetryKind::Failure));
	}
}
