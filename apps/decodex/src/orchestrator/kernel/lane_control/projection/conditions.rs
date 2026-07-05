use crate::orchestrator::kernel::{
	lane_control::model::LaneControlKernelInput,
	state::{LivenessState, OwnershipState, PolicyState},
};

pub(in crate::orchestrator::kernel::lane_control::projection) fn lane_control_conditions(
	input: &LaneControlKernelInput<'_>,
	ownership: OwnershipState,
	liveness: LivenessState,
	policy: PolicyState,
) -> Vec<&'static str> {
	let mut conditions = Vec::new();

	if !input.run_lease && input.attempt_active {
		conditions.push("run_lease_missing");
	}
	if input.attempt_terminal
		&& matches!(
			liveness,
			LivenessState::ProcessAlive
				| LivenessState::ThreadActive
				| LivenessState::ProtocolRecent
		) {
		conditions.push("terminal_attempt_has_live_evidence");
	}
	if liveness == LivenessState::HostBootMismatch {
		conditions.push("host_boot_id_mismatch");
	}
	if policy == PolicyState::ReviewChurnExceeded {
		conditions.push("review_churn_threshold_exceeded");
	}
	if policy == PolicyState::ContinuationRecoveryChurnExceeded {
		conditions.push("continuation_recovery_budget_exceeded");
	}
	if liveness == LivenessState::LateProtocolActivity {
		conditions.push("late_protocol_activity_after_terminal_barrier");
	}
	if matches!(
		policy,
		PolicyState::AuthorityBoundaryRequired | PolicyState::HumanAttentionRequired
	) {
		conditions.push("policy_requires_human_attention");
	}
	if ownership == OwnershipState::LeasedRun && !input.run_lease {
		conditions.push("invalid_leased_run_without_lease");
	}

	conditions
}
