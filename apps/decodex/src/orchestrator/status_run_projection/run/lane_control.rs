use crate::orchestrator::{
	CONTINUATION_PENDING_RUN_STATUS, OperatorLaneControlProjection, OperatorRunStatus,
	REVIEW_POLICY_CONVERGENCE_BUDGET,
	kernel::{
		lane_control::{LaneControlKernelInput, project_lane_control},
		state::PolicyState,
	},
};

pub(super) fn hydrate_operator_run_derived_status(
	mut status: OperatorRunStatus,
) -> OperatorRunStatus {
	let lane_control_state = operator_lane_control_state(&status);

	status.has_fresh_execution = lane_control_state.has_fresh_execution;
	status.ownership_state = lane_control_state.projection.ownership_state;
	status.liveness_state = lane_control_state.projection.liveness_state;
	status.policy_state = lane_control_state.projection.policy_state;
	status.terminalization_state = lane_control_state.projection.terminalization_state;
	status.lane_control_next_action = lane_control_state.projection.next_action;
	status.lane_control_conditions = lane_control_state.projection.conditions;
	status.needs_attention = lane_control_state.counts_as_attention;
	status.counts_as_running = lane_control_state.counts_as_running;

	status
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct OperatorRunLaneControlReadback {
	pub(in crate::orchestrator) has_live_execution: bool,
	pub(in crate::orchestrator) has_authoritative_live_owner: bool,
	pub(in crate::orchestrator) counts_as_current_lane: bool,
	pub(in crate::orchestrator) counts_as_running: bool,
	pub(in crate::orchestrator) counts_as_attention: bool,
}

struct OperatorRunLaneControlState {
	projection: OperatorLaneControlProjection,
	has_fresh_execution: bool,
	readback: OperatorRunLaneControlReadback,
	counts_as_attention: bool,
	counts_as_running: bool,
}

pub(in crate::orchestrator) fn operator_run_lane_control_readback(
	run: &OperatorRunStatus,
) -> OperatorRunLaneControlReadback {
	operator_lane_control_state(run).readback
}

fn operator_lane_control_state(run: &OperatorRunStatus) -> OperatorRunLaneControlState {
	let projection = project_lane_control(&operator_lane_control_kernel_input(run));

	OperatorRunLaneControlState {
		projection: OperatorLaneControlProjection {
			ownership_state: projection.axes.ownership.as_str().to_owned(),
			liveness_state: projection.axes.liveness.as_str().to_owned(),
			policy_state: projection.axes.policy.as_str().to_owned(),
			terminalization_state: projection.axes.terminalization.as_str().to_owned(),
			next_action: projection.next_action,
			conditions: projection.conditions.into_iter().map(str::to_owned).collect(),
		},
		has_fresh_execution: projection.has_fresh_execution,
		readback: OperatorRunLaneControlReadback {
			has_live_execution: projection.has_live_execution,
			has_authoritative_live_owner: projection.has_authoritative_live_owner,
			counts_as_current_lane: projection.counts_as_current_lane,
			counts_as_running: projection.counts_as_running,
			counts_as_attention: projection.counts_as_attention,
		},
		counts_as_attention: projection.counts_as_attention,
		counts_as_running: projection.counts_as_running,
	}
}

fn operator_lane_control_kernel_input(run: &OperatorRunStatus) -> LaneControlKernelInput<'_> {
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
		protocol_recent: operator_run_has_recent_app_server_execution(run),
		suspected_stall: run.suspected_stall,
		stale_execution_without_known_process:
			operator_run_has_stale_execution_without_known_process(run),
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
				.is_ok_and(|idle_for| idle_for < crate::agent::RUN_LEASE_IDLE_TIMEOUT.as_secs())
		})
}

fn operator_run_has_stale_execution_without_known_process(run: &OperatorRunStatus) -> bool {
	matches!(run.status.as_str(), "starting" | "running")
		&& run.phase == "executing"
		&& run.wait_reason.is_none()
		&& run.process_alive != Some(true)
		&& !operator_run_has_recent_app_server_execution(run)
		&& [run.idle_for_seconds, run.protocol_idle_for_seconds].iter().any(|idle_for| {
			idle_for.is_some_and(|idle_for| {
				u64::try_from(idle_for).is_ok_and(|idle_for| {
					idle_for >= crate::agent::RUN_LEASE_IDLE_TIMEOUT.as_secs()
				})
			})
		})
}
