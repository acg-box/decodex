use crate::orchestrator::{
	self, IssueDispatchMode, LaneDecisionSnapshot, LaneNextAction, PhaseGoalKind,
	RepoGateFailureDisposition, RepoGateFailureSignal, RetryKind, kernel::action::OwnedLaneAction,
};

fn repo_gate_snapshot(disposition: RepoGateFailureDisposition) -> LaneDecisionSnapshot {
	LaneDecisionSnapshot::repo_gate_failure(
		"PUB-101",
		"run-1",
		1,
		IssueDispatchMode::Normal,
		PhaseGoalKind::ImplementToValidationReady,
		RepoGateFailureSignal::new(disposition, "repo_gate_verify_failed", false),
	)
}

#[test]
fn validation_evidence_failure_projects_kernel_retry_to_legacy_retry_failure() {
	let snapshot = LaneDecisionSnapshot::validation_evidence(
		"PUB-101",
		"run-1",
		1,
		IssueDispatchMode::Normal,
		PhaseGoalKind::ImplementToValidationReady,
		0,
		false,
		false,
	);
	let decision = orchestrator::decide_lane_next_action(&snapshot);

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
	let decision = orchestrator::decide_lane_next_action(&snapshot);

	assert_eq!(decision.next_action, LaneNextAction::WaitExternal);
	assert_eq!(decision.kernel_decision.decision_class, OwnedLaneAction::WaitForExternalSignal);
	assert_eq!(decision.kernel_decision.command_intents[0].kind.as_str(), "wait_external");
	assert!(!decision.permits_phase_repair_retry());
}

#[test]
fn repo_gate_continue_repair_requires_kernel_retry_intent() {
	let snapshot = repo_gate_snapshot(RepoGateFailureDisposition::ContinueRepair);
	let decision = orchestrator::decide_lane_next_action(&snapshot);

	assert_eq!(decision.next_action, LaneNextAction::RetryFailure);
	assert_eq!(decision.kernel_decision.decision_class, OwnedLaneAction::RetryAutomatically);
	assert!(decision.permits_phase_repair_retry());
}

#[test]
fn scope_envelope_violation_projects_kernel_manual_to_legacy_attention() {
	let mut snapshot = repo_gate_snapshot(RepoGateFailureDisposition::NeedsHumanAttention);

	snapshot.scope_envelope_violation = true;

	let decision = orchestrator::decide_lane_next_action(&snapshot);

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
	let decision = orchestrator::decide_lane_next_action(&snapshot);

	assert_eq!(decision.next_action, LaneNextAction::ResumeContinuation);
	assert_eq!(decision.kernel_decision.decision_class, OwnedLaneAction::ResumeRetainedLane);
	assert_eq!(decision.kernel_decision.command_intents[0].kind.as_str(), "resume_retained_lane");
	assert!(decision.permits_child_exit_retry_kind(RetryKind::Continuation));
	assert!(!decision.permits_child_exit_retry_kind(RetryKind::Failure));
}
