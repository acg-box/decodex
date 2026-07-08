use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		CONTINUATION_PENDING_RUN_STATUS, OperatorRunStatus, REVIEW_POLICY_CONVERGENCE_BUDGET,
		kernel::{lane_control::LaneControlKernelInput, state::PolicyState},
	},
};

pub(in crate::orchestrator::status::run_projection::run::lane_control) fn operator_lane_control_kernel_input(
	run: &OperatorRunStatus,
) -> LaneControlKernelInput<'_> {
	LaneControlKernelInput {
		run_lease: run.run_lease,
		attempt_active: matches!(
			run.attempt_status.as_str(),
			"starting" | "running" | "continuation_pending"
		),
		attempt_terminal: matches!(
			run.attempt_status.as_str(),
			"failed" | "interrupted" | "stalled" | "succeeded"
		),
		status_starting_or_running: matches!(run.status.as_str(), "starting" | "running"),
		status_needs_attention_or_terminal_failure: matches!(
			run.status.as_str(),
			"needs_attention" | "terminal_failure"
		),
		phase_executing: run.phase == "executing",
		phase_needs_attention: run.phase == "needs_attention",
		phase_stalled: run.phase == "stalled",
		phase_terminal: matches!(run.phase.as_str(), "completed" | "failed" | "terminated"),
		cleanup_complete_signal: matches!(
			run.status.as_str(),
			"cleanup_complete" | "merged_closeout_reconciled"
		),
		current_operation_ledger_outcome: run.current_operation == "ledger_outcome",
		wait_reason_present: run.wait_reason.is_some(),
		continuation_wait: operator_run_is_continuation_wait(run),
		process_alive: run.process_alive,
		execution_liveness_observed: matches!(
			run.execution_liveness.as_str(),
			"process_alive" | "thread_active" | "protocol_observed"
		),
		host_boot_mismatch: matches!(
			run.process_liveness_reason.as_deref(),
			Some("host_boot_id_mismatch")
		),
		not_running_signal: matches!(
			run.execution_liveness.as_str(),
			"not_running" | "process_identity_mismatch"
		),
		thread_active: matches!(run.thread_status.as_deref(), Some("active"))
			|| !run.thread_active_flags.is_empty(),
		thread_terminal_failure: operator_run_thread_terminal_failure(run),
		protocol_recent: operator_run_has_recent_app_server_execution(run),
		suspected_stall: run.suspected_stall,
		stale_execution_without_known_process: operator_run_has_stale_execution_without_process(
			run,
		),
		policy: operator_run_policy_state(run),
		loop_next_action: run
			.loop_status
			.as_ref()
			.and_then(|loop_status| loop_status.next_action.as_deref()),
	}
}

fn operator_run_is_continuation_wait(run: &OperatorRunStatus) -> bool {
	run.attempt_status == CONTINUATION_PENDING_RUN_STATUS
		|| run.phase == "waiting_continuation"
		|| run.retry_kind.as_deref() == Some("continuation")
		|| run.wait_reason.as_deref() == Some("continuation_retry")
}

fn operator_run_policy_state(run: &OperatorRunStatus) -> PolicyState {
	if run.continuation_recovery.as_ref().is_some_and(|recovery| recovery.budget_exceeded) {
		return PolicyState::ContinuationRecoveryChurnExceeded;
	}

	let Some(loop_status) = run.loop_status.as_ref() else {
		return PolicyState::Allowed;
	};

	if loop_status.decision_request.is_some() {
		return PolicyState::AuthorityBoundaryRequired;
	}
	if loop_status.autonomy == "human_required" {
		return PolicyState::HumanAttentionRequired;
	}

	if let Some(recovery) = loop_status.architecture_recovery.as_ref() {
		return if recovery.status == "active" {
			PolicyState::ArchitectureRecoveryPending
		} else {
			PolicyState::HumanAttentionRequired
		};
	}
	if let Some(review) = loop_status.review.as_ref() {
		return match review.status.as_str() {
			"pending" => PolicyState::ReviewPending,
			"findings" => {
				if review.checkpoint.as_ref().is_some_and(|checkpoint| {
					checkpoint.nonclean_rounds >= REVIEW_POLICY_CONVERGENCE_BUDGET
				}) {
					PolicyState::ReviewChurnExceeded
				} else {
					PolicyState::ReviewFindings
				}
			},
			"blocked" | "needs_architecture_review" => PolicyState::HumanAttentionRequired,
			_ => PolicyState::Allowed,
		};
	}

	PolicyState::Allowed
}

fn operator_run_has_recent_app_server_execution(run: &OperatorRunStatus) -> bool {
	matches!(run.thread_status.as_deref(), Some("active"))
		|| !run.thread_active_flags.is_empty()
		|| run.protocol_idle_for_seconds.is_some_and(|idle_for| {
			u64::try_from(idle_for)
				.is_ok_and(|idle_for| idle_for < RUN_LEASE_IDLE_TIMEOUT.as_secs())
		})
}

fn operator_run_thread_terminal_failure(run: &OperatorRunStatus) -> bool {
	matches!(run.thread_status.as_deref(), Some("systemError" | "failed" | "interrupted"))
		&& matches!(run.status.as_str(), "starting" | "running")
		&& run.phase == "executing"
}

fn operator_run_has_stale_execution_without_process(run: &OperatorRunStatus) -> bool {
	matches!(run.status.as_str(), "starting" | "running")
		&& run.phase == "executing"
		&& run.wait_reason.is_none()
		&& run.process_alive != Some(true)
		&& !operator_run_has_recent_app_server_execution(run)
		&& [run.idle_for_seconds, run.protocol_idle_for_seconds].iter().any(|idle_for| {
			idle_for.is_some_and(|idle_for| {
				u64::try_from(idle_for)
					.is_ok_and(|idle_for| idle_for >= RUN_LEASE_IDLE_TIMEOUT.as_secs())
			})
		})
}
