mod accessors;
mod builder;
mod lane_control;
mod lifecycle;
mod phase;
mod status;

use crate::{
	orchestrator::{
		AgentPrivateEvidenceRef, CodexAccountActivitySummary, OperatorLoopStatus,
		OperatorRunAppServerState, OperatorRunLifecycleProjection, OperatorRunProtocolSummary,
		OperatorRunStatus, OperatorRunTiming, OperatorTerminalFinalizeProjection,
		OperatorValidationEvidenceStatus, PrivateExecutionEvent, ProjectLoopEvidenceSnapshot,
		ProjectRunStatus, ProtocolActivitySummary, RunActivityMarker, ServiceConfig,
	},
	prelude::Result,
};
use lane_control::OperatorRunLaneControlReadback;

pub(crate) fn operator_run_status(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	project_display_name: &str,
	run: ProjectRunStatus,
	now_unix_epoch: i64,
) -> Result<OperatorRunStatus> {
	builder::operator_run_status(project, loop_evidence, project_display_name, run, now_unix_epoch)
}

pub(crate) fn operator_run_active_goal_phase(events: &[PrivateExecutionEvent]) -> Option<String> {
	phase::operator_run_active_goal_phase(events)
}

pub(crate) fn operator_run_public_progress_phase(
	events: &[PrivateExecutionEvent],
) -> Option<String> {
	phase::operator_run_public_progress_phase(events)
}

pub(crate) fn operator_run_validation_evidence_status(
	events: &[PrivateExecutionEvent],
) -> Option<OperatorValidationEvidenceStatus> {
	phase::operator_run_validation_evidence_status(events)
}

pub(crate) fn hydrate_operator_run_derived_status(status: OperatorRunStatus) -> OperatorRunStatus {
	lane_control::hydrate_operator_run_derived_status(status)
}

pub(crate) fn operator_run_lane_control_readback(
	run: &OperatorRunStatus,
) -> OperatorRunLaneControlReadback {
	lane_control::operator_run_lane_control_readback(run)
}

pub(crate) fn operator_run_lifecycle_projection(
	run: &ProjectRunStatus,
	marker: Option<&RunActivityMarker>,
	terminal_finalize_projection: Option<OperatorTerminalFinalizeProjection>,
	timing: &OperatorRunTiming,
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
	now_unix_epoch: i64,
) -> OperatorRunLifecycleProjection {
	lifecycle::operator_run_lifecycle_projection(
		run,
		marker,
		terminal_finalize_projection,
		timing,
		app_server_state,
		protocol_summary,
		now_unix_epoch,
	)
}

pub(crate) fn operator_run_wait_reason(
	phase: &str,
	wait_reason: Option<String>,
	protocol_activity: Option<&ProtocolActivitySummary>,
) -> Option<String> {
	phase::operator_run_wait_reason(phase, wait_reason, protocol_activity)
}

pub(crate) fn operator_run_accounts(
	marker: Option<&RunActivityMarker>,
) -> (Option<CodexAccountActivitySummary>, Vec<CodexAccountActivitySummary>) {
	accessors::operator_run_accounts(marker)
}

pub(crate) fn operator_run_relative_worktree_path(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
) -> Option<String> {
	accessors::operator_run_relative_worktree_path(project, run)
}

pub(crate) fn operator_run_private_evidence(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
) -> AgentPrivateEvidenceRef {
	accessors::operator_run_private_evidence(project, run, issue_identifier)
}

pub(crate) fn operator_run_loop_status(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
	status: &str,
	phase: &str,
	current_operation: &str,
) -> Result<OperatorLoopStatus> {
	accessors::operator_run_loop_status(
		project,
		loop_evidence,
		run,
		status,
		phase,
		current_operation,
	)
}

pub(crate) fn operator_run_default_review_phase(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> Option<&'static str> {
	phase::operator_run_default_review_phase(status, phase, current_operation)
}

pub(crate) fn operator_run_lifecycle_loop_summary(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> Option<String> {
	phase::operator_run_lifecycle_loop_summary(status, phase, current_operation)
}
