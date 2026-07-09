use crate::orchestrator::{
	IssueDispatchMode, PhaseGoalKind, RepoGateFailureDisposition, RetryKind,
	kernel::decision::OwnedLaneDecision,
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
	pub(in crate::orchestrator) validation_evidence_failure: bool,
	pub(in crate::orchestrator) repo_gate_disposition: Option<RepoGateFailureDisposition>,
	pub(in crate::orchestrator) repo_gate_error_class: Option<&'static str>,
	pub(in crate::orchestrator) scope_envelope_violation: bool,
	pub(in crate::orchestrator) ambiguous_lineage: bool,
	pub(in crate::orchestrator) terminal_evidence_present: bool,
}
impl LaneDecisionSnapshot {
	#[allow(clippy::too_many_arguments)]
	pub(in crate::orchestrator) fn validation_evidence(
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
			validation_evidence_failure: true,
			repo_gate_disposition: None,
			repo_gate_error_class: None,
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
			validation_evidence_failure: false,
			repo_gate_disposition: None,
			repo_gate_error_class: None,
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
		repo_gate_failure: RepoGateFailureSignal,
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
			validation_evidence_failure: false,
			repo_gate_disposition: Some(repo_gate_failure.disposition),
			repo_gate_error_class: Some(repo_gate_failure.error_class),
			scope_envelope_violation: repo_gate_failure.scope_envelope_violation,
			ambiguous_lineage: false,
			terminal_evidence_present: false,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct RepoGateFailureSignal {
	pub(in crate::orchestrator) disposition: RepoGateFailureDisposition,
	pub(in crate::orchestrator) error_class: &'static str,
	pub(in crate::orchestrator) scope_envelope_violation: bool,
}
impl RepoGateFailureSignal {
	pub(in crate::orchestrator) const fn new(
		disposition: RepoGateFailureDisposition,
		error_class: &'static str,
		scope_envelope_violation: bool,
	) -> Self {
		Self { disposition, error_class, scope_envelope_violation }
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
}
