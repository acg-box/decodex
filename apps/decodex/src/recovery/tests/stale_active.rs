mod claim_labels;
mod evidence_blockers;
mod reentry;
mod release;
mod support;
mod telemetry;

pub(in crate::recovery::tests::stale_active) use self::support::{
	append_app_server_no_progress_failure_evidence,
	append_dead_process_interrupt_control_telemetry, append_harness_outcome_with_pr_progress,
	append_harness_outcome_with_review_progress, append_harness_outcome_with_validation_progress,
	append_no_diff_guardrail_event, append_no_diff_guardrail_event_with_source_error_class,
	append_phase_goal_recovery_event, append_stale_active_release_audit,
	append_stale_active_release_audit_for_run, seed_dead_orphan_runtime_telemetry,
	seed_dead_orphan_runtime_telemetry_without_control_channel, seed_lane_claim,
};
pub(in crate::recovery::tests::stale_active) use crate::recovery::{
	apply_stale_active_release_with_tracker, diagnose_stale_active_issues,
};
