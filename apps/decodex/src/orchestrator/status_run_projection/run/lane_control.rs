use crate::orchestrator::status_summary;
use crate::orchestrator::{
	CONTINUATION_PENDING_RUN_STATUS, OperatorLaneControlProjection, OperatorRunStatus,
	REVIEW_POLICY_CONVERGENCE_BUDGET,
};

pub(super) fn hydrate_operator_run_derived_status(
	mut status: OperatorRunStatus,
) -> OperatorRunStatus {
	status.has_fresh_execution = status_summary::operator_run_has_fresh_execution(&status);
	status.needs_attention = status_summary::operator_run_needs_attention(&status);

	let lane_control_state = operator_lane_control_state(&status);

	status.ownership_state = lane_control_state.ownership_state;
	status.liveness_state = lane_control_state.liveness_state;
	status.policy_state = lane_control_state.policy_state;
	status.terminalization_state = lane_control_state.terminalization_state;
	status.lane_control_next_action = lane_control_state.next_action;
	status.lane_control_conditions = lane_control_state.conditions;
	status.needs_attention = status_summary::operator_run_counts_as_attention(&status);
	status.counts_as_running = status_summary::operator_run_counts_as_running(&status);

	status
}

pub(super) fn operator_lane_control_state(
	run: &OperatorRunStatus,
) -> OperatorLaneControlProjection {
	let liveness_state = operator_run_liveness_state(run);
	let policy_state = operator_run_policy_state(run);
	let terminalization_state = operator_run_terminalization_state(run, &liveness_state);
	let ownership_state =
		operator_run_ownership_state(run, &liveness_state, &policy_state, &terminalization_state);
	let next_action = operator_run_lane_control_next_action(
		run,
		&ownership_state,
		&liveness_state,
		&policy_state,
		&terminalization_state,
	);
	let mut conditions = operator_run_lane_control_conditions(run, &liveness_state, &policy_state);

	if ownership_state == "leased_run" && !run.run_lease {
		conditions.push(String::from("invalid_leased_run_without_lease"));
	}

	OperatorLaneControlProjection {
		ownership_state,
		liveness_state,
		policy_state,
		terminalization_state,
		next_action,
		conditions,
	}
}

pub(super) fn operator_run_ownership_state(
	run: &OperatorRunStatus,
	liveness_state: &str,
	policy_state: &str,
	terminalization_state: &str,
) -> String {
	if run.run_lease
		&& matches!(run.attempt_status.as_str(), "starting" | "running" | "continuation_pending")
		&& !matches!(
			policy_state,
			"review_churn_exceeded"
				| "continuation_recovery_churn_exceeded"
				| "authority_boundary_required"
				| "human_attention_required"
		) {
		return String::from("leased_run");
	}
	if matches!(
		policy_state,
		"review_churn_exceeded"
			| "continuation_recovery_churn_exceeded"
			| "authority_boundary_required"
			| "human_attention_required"
	) || run.needs_attention
		|| (!run.run_lease && liveness_state == "host_boot_mismatch")
	{
		return String::from("retained_attention");
	}
	if operator_run_is_continuation_wait(run) {
		return String::from("continuation_pending");
	}
	if !run.run_lease
		&& matches!(liveness_state, "process_alive" | "thread_active" | "protocol_recent")
	{
		return String::from("orphaned_live_thread");
	}
	if terminalization_state != "none" && terminalization_state != "cleanup_complete" {
		return String::from("terminalizing");
	}
	if matches!(run.attempt_status.as_str(), "starting" | "running" | "continuation_pending") {
		return String::from("pending");
	}

	String::from("closed")
}

pub(super) fn operator_run_is_continuation_wait(run: &OperatorRunStatus) -> bool {
	run.attempt_status == CONTINUATION_PENDING_RUN_STATUS
		|| run.phase == "waiting_continuation"
		|| run.retry_kind.as_deref() == Some("continuation")
		|| run.wait_reason.as_deref() == Some("continuation_retry")
}

pub(super) fn operator_run_liveness_state(run: &OperatorRunStatus) -> String {
	if matches!(run.process_liveness_reason.as_deref(), Some("host_boot_id_mismatch")) {
		return String::from("host_boot_mismatch");
	}
	if run.process_alive == Some(true) {
		return String::from("process_alive");
	}
	if run.process_alive == Some(false)
		|| matches!(run.execution_liveness.as_str(), "not_running" | "process_identity_mismatch")
	{
		return String::from("not_running");
	}
	if matches!(run.thread_status.as_deref(), Some("active")) || !run.thread_active_flags.is_empty()
	{
		return String::from("thread_active");
	}
	if status_summary::operator_run_has_recent_app_server_execution(run) {
		return String::from("protocol_recent");
	}

	String::from("unknown")
}

pub(super) fn operator_run_policy_state(run: &OperatorRunStatus) -> String {
	if run.continuation_recovery.as_ref().is_some_and(|recovery| recovery.budget_exceeded) {
		return String::from("continuation_recovery_churn_exceeded");
	}

	let Some(loop_status) = run.loop_status.as_ref() else {
		return String::from("allowed");
	};

	if loop_status.decision_request.is_some() {
		return String::from("authority_boundary_required");
	}
	if loop_status.autonomy == "human_required" {
		return String::from("human_attention_required");
	}

	if let Some(recovery) = loop_status.architecture_recovery.as_ref() {
		return if recovery.status == "active" {
			String::from("architecture_recovery_pending")
		} else {
			String::from("human_attention_required")
		};
	}
	if let Some(review) = loop_status.review.as_ref() {
		return match review.status.as_str() {
			"pending" => String::from("review_pending"),
			"findings" => {
				if review.checkpoint.as_ref().is_some_and(|checkpoint| {
					checkpoint.nonclean_rounds >= REVIEW_POLICY_CONVERGENCE_BUDGET
				}) {
					String::from("review_churn_exceeded")
				} else {
					String::from("review_findings")
				}
			},
			"blocked" | "needs_architecture_review" => String::from("human_attention_required"),
			_ => String::from("allowed"),
		};
	}

	String::from("allowed")
}

pub(super) fn operator_run_terminalization_state(
	run: &OperatorRunStatus,
	liveness_state: &str,
) -> String {
	if matches!(run.status.as_str(), "cleanup_complete" | "merged_closeout_reconciled")
		|| matches!(run.current_operation.as_str(), "ledger_outcome")
			&& matches!(run.phase.as_str(), "completed")
	{
		return String::from("cleanup_complete");
	}
	if matches!(run.phase.as_str(), "completed" | "failed" | "terminated")
		&& !run.run_lease
		&& matches!(liveness_state, "not_running" | "unknown")
	{
		return String::from("cleanup_complete");
	}
	if matches!(run.phase.as_str(), "completed" | "failed" | "terminated") {
		return String::from("barrier_started");
	}

	String::from("none")
}

pub(super) fn operator_run_lane_control_conditions(
	run: &OperatorRunStatus,
	liveness_state: &str,
	policy_state: &str,
) -> Vec<String> {
	let mut conditions = Vec::new();

	if !run.run_lease
		&& matches!(run.attempt_status.as_str(), "starting" | "running" | "continuation_pending")
	{
		conditions.push(String::from("run_lease_missing"));
	}
	if matches!(run.attempt_status.as_str(), "failed" | "interrupted" | "stalled" | "succeeded")
		&& matches!(liveness_state, "process_alive" | "thread_active" | "protocol_recent")
	{
		conditions.push(String::from("terminal_attempt_has_live_evidence"));
	}
	if liveness_state == "host_boot_mismatch" {
		conditions.push(String::from("host_boot_id_mismatch"));
	}
	if policy_state == "review_churn_exceeded" {
		conditions.push(String::from("review_churn_threshold_exceeded"));
	}
	if policy_state == "continuation_recovery_churn_exceeded" {
		conditions.push(String::from("continuation_recovery_budget_exceeded"));
	}
	if matches!(policy_state, "authority_boundary_required" | "human_attention_required") {
		conditions.push(String::from("policy_requires_human_attention"));
	}

	conditions
}

pub(super) fn operator_run_lane_control_next_action(
	run: &OperatorRunStatus,
	ownership_state: &str,
	liveness_state: &str,
	policy_state: &str,
	terminalization_state: &str,
) -> String {
	if policy_state == "review_churn_exceeded" {
		return String::from("start_architecture_recovery_or_stop_for_human_attention");
	}
	if policy_state == "continuation_recovery_churn_exceeded" {
		return String::from("stop_auto_continuation_and_request_architecture_recovery");
	}
	if matches!(policy_state, "authority_boundary_required" | "human_attention_required") {
		return String::from("resolve_policy_stop_before_mutating_lane");
	}
	if ownership_state == "orphaned_live_thread" {
		return String::from("inspect_or_interrupt_orphaned_live_thread");
	}
	if liveness_state == "host_boot_mismatch" {
		return String::from("inspect_recovery_evidence");
	}
	if terminalization_state != "none" && terminalization_state != "cleanup_complete" {
		return String::from("finish_terminalization");
	}
	if ownership_state == "leased_run" {
		if let Some(next_action) =
			run.loop_status.as_ref().and_then(|loop_status| loop_status.next_action.clone())
		{
			return next_action;
		}

		return String::from("continue_owned_attempt");
	}
	if ownership_state == "continuation_pending" {
		return String::from("wait_for_continuation_reentry");
	}
	if ownership_state == "closed" {
		return String::from("no_action");
	}

	if let Some(next_action) =
		run.loop_status.as_ref().and_then(|loop_status| loop_status.next_action.clone())
	{
		return next_action;
	}

	String::from("inspect_lane_state")
}
