use crate::orchestrator::{
	OperatorRunStatus,
	agent_evidence::AgentRunDiagnosis,
	kernel::state::{OwnershipState, PolicyState},
	status_summary,
};

pub(crate) fn agent_run_blocker_reason(run: &OperatorRunStatus) -> Option<&'static str> {
	match PolicyState::from_str(&run.policy_state) {
		Some(PolicyState::ReviewChurnExceeded) => return Some("review_churn_exceeded"),
		Some(PolicyState::RuntimeRecoveryRequired) => return Some("runtime_recovery_required"),
		Some(PolicyState::RuntimeRecoveryBlocked) => return Some("runtime_recovery_blocked"),
		_ => {},
	}
	match OwnershipState::from_str(&run.ownership_state) {
		Some(OwnershipState::RetainedAttention) => return Some("retained_attention"),
		Some(OwnershipState::OrphanedLiveThread) => return Some("orphaned_live_thread"),
		Some(OwnershipState::Terminalizing) => return Some("terminalizing"),
		Some(OwnershipState::GhostLane) => return Some("ghost_lane"),
		_ => {},
	}

	if run.suspected_stall {
		return Some("suspected_stall");
	}
	if run.phase == "stalled" {
		return Some("run_stalled");
	}
	if run.process_alive == Some(false) && matches!(run.status.as_str(), "starting" | "running") {
		return Some("process_exited_without_terminal_status");
	}
	if status_summary::operator_run_has_stale_execution_without_known_process(run) {
		return Some("stale_execution_without_known_process");
	}

	None
}

pub(crate) fn agent_run_next_action(run: &OperatorRunStatus) -> Option<String> {
	if !run.lane_control_next_action.trim().is_empty() {
		return Some(run.lane_control_next_action.clone());
	}

	match agent_run_blocker_reason(run) {
		Some("suspected_stall" | "run_stalled" | "stale_execution_without_known_process") => {
			Some(String::from(
				"Inspect the run capsule, retained worktree, protocol activity, and process state before retrying.",
			))
		},
		Some("process_exited_without_terminal_status") => Some(String::from(
			"Inspect the retained worktree and runtime markers; reconcile or retry only after preserving useful local changes.",
		)),
		_ => None,
	}
}

pub(in crate::orchestrator::agent_evidence::capsules::runs) fn agent_run_diagnosis(
	run: &OperatorRunStatus,
) -> AgentRunDiagnosis {
	let reason = agent_run_blocker_reason(run);

	AgentRunDiagnosis {
		attention_required: reason.is_some(),
		reason_code: reason.map(str::to_owned),
		next_action: agent_run_next_action(run),
	}
}
