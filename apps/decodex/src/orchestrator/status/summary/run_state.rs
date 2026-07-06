use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		OperatorRunStatus,
		kernel::state::{OwnershipState, PolicyState},
		status_run_projection::{self},
	},
};

pub(super) fn operator_run_counts_as_waiting(run: &OperatorRunStatus) -> bool {
	run.phase == "retry_backoff" || run.phase == "waiting_continuation" || run.wait_reason.is_some()
}

pub(super) fn operator_run_counts_as_current_lane(run: &OperatorRunStatus) -> bool {
	status_run_projection::operator_run_lane_control_readback(run).counts_as_current_lane
}

pub(super) fn operator_run_has_live_execution(run: &OperatorRunStatus) -> bool {
	status_run_projection::operator_run_lane_control_readback(run).has_live_execution
}

pub(super) fn operator_run_counts_as_running(run: &OperatorRunStatus) -> bool {
	if !run.ownership_state.is_empty() {
		return run.counts_as_running;
	}

	status_run_projection::operator_run_lane_control_readback(run).counts_as_running
}

pub(super) fn operator_run_counts_as_attention(run: &OperatorRunStatus) -> bool {
	let ownership = OwnershipState::from_str(&run.ownership_state);
	let policy = PolicyState::from_str(&run.policy_state);

	if !run.ownership_state.is_empty() {
		return run.needs_attention
			|| ownership == Some(OwnershipState::RetainedAttention)
			|| policy_requires_attention(policy);
	}

	status_run_projection::operator_run_lane_control_readback(run).counts_as_attention
}

pub(super) fn operator_run_has_recent_app_server_execution(run: &OperatorRunStatus) -> bool {
	matches!(run.thread_status.as_deref(), Some("active"))
		|| !run.thread_active_flags.is_empty()
		|| run.protocol_idle_for_seconds.is_some_and(|idle_for| {
			u64::try_from(idle_for)
				.is_ok_and(|idle_for| idle_for < RUN_LEASE_IDLE_TIMEOUT.as_secs())
		})
}

pub(super) fn operator_run_has_stale_execution_without_known_process(
	run: &OperatorRunStatus,
) -> bool {
	matches!(run.status.as_str(), "starting" | "running")
		&& run.phase == "executing"
		&& run.wait_reason.is_none()
		&& run.process_alive != Some(true)
		&& !run.has_fresh_execution
		&& [run.idle_for_seconds, run.protocol_idle_for_seconds].iter().any(|idle_for| {
			idle_for.is_some_and(|idle_for| {
				u64::try_from(idle_for)
					.is_ok_and(|idle_for| idle_for >= RUN_LEASE_IDLE_TIMEOUT.as_secs())
			})
		})
}

fn policy_requires_attention(policy: Option<PolicyState>) -> bool {
	matches!(
		policy,
		Some(
			PolicyState::ReviewChurnExceeded
				| PolicyState::ContinuationRecoveryChurnExceeded
				| PolicyState::AuthorityBoundaryRequired
				| PolicyState::HumanAttentionRequired
		)
	)
}
