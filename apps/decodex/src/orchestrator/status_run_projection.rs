//! Operator run status projection, protocol/activity readback, and lane lifecycle metrics.

mod history;
mod loop_status;
mod run;
mod runtime;

#[allow(unused_imports)]
pub(super) use self::{
	history::{
		current_lane_lifecycle_attempts, hydrate_current_lane_lifecycle_metrics,
		hydrate_history_lane_from_run, issue_identifier_from_run_id, issue_identifier_in_text,
		operator_history_lanes, operator_lane_lifecycle_attempt_evidence,
		operator_lane_lifecycle_metrics, operator_lane_lifecycle_phase_metrics,
		operator_lane_lifecycle_totals, operator_lifecycle_metric_phase,
		operator_run_current_lane_snapshot_attempt, operator_run_group_key,
		operator_run_issue_identifier_from_fields, operator_run_issue_key,
		operator_run_lifecycle_metric_phase,
	},
	loop_status::{
		operator_architecture_recovery_next_action,
		operator_architecture_recovery_status_for_reason,
		operator_architecture_recovery_status_from_event, operator_boundary_policy_blocks_landing,
		operator_boundary_policy_decision_from_disposition,
		operator_boundary_policy_requires_enhanced_evidence, operator_boundary_status_from_event,
		operator_latest_review_checkpoint_event_status, operator_loop_autonomy,
		operator_loop_status_for_run, operator_loop_status_for_run_with_evidence,
		operator_loop_status_next_action, operator_loop_status_summary,
		operator_review_checkpoint_summary_fields, operator_review_loop_status,
	},
	run::{
		hydrate_operator_run_derived_status, operator_run_accounts, operator_run_active_goal_phase,
		operator_run_default_review_phase, operator_run_lane_control_readback,
		operator_run_lifecycle_loop_summary, operator_run_lifecycle_projection,
		operator_run_loop_status, operator_run_phase_acceptance_status,
		operator_run_private_evidence, operator_run_public_progress_phase,
		operator_run_relative_worktree_path, operator_run_status, operator_run_status_from_parts,
		operator_run_wait_reason,
	},
	runtime::{
		classify_operator_run_operation, classify_operator_run_phase,
		contains_protocol_activity_host_path_shape, contains_protocol_activity_secret_shape,
		format_optional_i64, format_optional_unix_timestamp, idle_duration_seconds,
		is_high_entropy_protocol_activity_token, load_operator_run_marker,
		marker_protocol_summary_supersedes_run, max_optional_i64,
		operator_continuation_recovery_event_status,
		operator_latest_repo_gate_failure_progress_diagnostic,
		operator_protocol_activity_detail_is_public,
		operator_protocol_event_counts_as_live_execution,
		operator_repo_gate_failure_progress_diagnostic, operator_run_app_server_state,
		operator_run_child_agent_activity, operator_run_continuation_recovery_status,
		operator_run_control_capability, operator_run_execution_liveness,
		operator_run_has_app_server_execution_evidence,
		operator_run_has_recent_protocol_execution_evidence, operator_run_is_suspected_stall,
		operator_run_live_evidence_source, operator_run_progress_diagnostic,
		operator_run_protocol_activity, operator_run_protocol_summary,
		operator_run_queue_lease_state, operator_run_status_projection_reason,
		operator_run_terminal_finalize_projection, operator_run_timing,
		operator_run_visible_status, process_liveness_reason_is_identity_mismatch,
		protocol_activity_is_non_work_only, protocol_activity_token_separator,
		protocol_wait_reason_from_child_bucket, review_handoff_terminal_finalize_wait_reason,
		review_repair_terminal_finalize_wait_reason, sanitize_operator_protocol_activity_summary,
		suspected_operator_run_stall_threshold, visible_operator_run_retry_schedule,
	},
};
