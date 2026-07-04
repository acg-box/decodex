mod accessors;
mod lane_control;
mod lifecycle;
mod phase;
mod status;

use crate::{
	orchestrator::{
		AgentPrivateEvidenceRef, CodexAccountActivitySummary, OperatorLoopStatus,
		OperatorPhaseAcceptanceStatus, OperatorRunAppServerState, OperatorRunLifecycleProjection,
		OperatorRunProtocolSummary, OperatorRunStatus, OperatorRunTiming,
		OperatorTerminalFinalizeProjection, PrivateExecutionEvent, ProjectLoopEvidenceSnapshot,
		ProjectRunStatus, ProtocolActivitySummary, RunActivityMarker, ServiceConfig,
		status_process_liveness,
	},
	prelude::Result,
};
use lane_control::OperatorRunLaneControlReadback;
use status::OperatorRunStatusParts;

pub(crate) fn operator_run_status(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	project_display_name: &str,
	run: ProjectRunStatus,
	now_unix_epoch: i64,
) -> Result<OperatorRunStatus> {
	let marker = super::load_operator_run_marker(&run)?;
	let timing = super::operator_run_timing(&run, marker.as_ref(), now_unix_epoch);
	let app_server_state = super::operator_run_app_server_state(&run, marker.as_ref());
	let protocol_summary = super::operator_run_protocol_summary(&run, marker.as_ref());
	let terminal_finalize_projection =
		super::operator_run_terminal_finalize_projection(loop_evidence, &run);
	let lifecycle = operator_run_lifecycle_projection(
		&run,
		marker.as_ref(),
		terminal_finalize_projection,
		&timing,
		&app_server_state,
		&protocol_summary,
		now_unix_epoch,
	);
	let child_agent_activity = super::operator_run_child_agent_activity(
		marker.as_ref(),
		run.child_agent_activity(),
		now_unix_epoch,
	);
	let protocol_activity = super::operator_run_protocol_activity(
		marker.as_ref(),
		run.protocol_activity(),
		&app_server_state,
		child_agent_activity.as_ref(),
		timing.protocol_idle_for_seconds,
		matches!(lifecycle.status.as_str(), "starting" | "running"),
	);
	let wait_reason = operator_run_wait_reason(
		&lifecycle.phase,
		lifecycle.wait_reason.clone(),
		protocol_activity.as_ref(),
	);
	let private_events =
		loop_evidence.private_events(run.issue_id(), run.run_id(), run.attempt_number());
	let progress_diagnostic = super::operator_run_progress_diagnostic(
		&lifecycle.phase,
		&timing,
		protocol_activity.as_ref(),
		private_events,
		now_unix_epoch,
		status_process_liveness::run_activity_idle_timeout(marker.as_ref()),
	);
	let (account, accounts) = operator_run_accounts(marker.as_ref());
	let branch_name = run.branch_name().map(str::to_owned);
	let worktree_path = operator_run_relative_worktree_path(project, &run);
	let issue_identifier = super::operator_run_issue_identifier_from_fields(
		run.run_id(),
		branch_name.as_deref(),
		worktree_path.as_deref(),
	);
	let private_evidence =
		operator_run_private_evidence(project, &run, issue_identifier.as_deref());
	let continuation_recovery =
		super::operator_run_continuation_recovery_status(loop_evidence, &run);
	let active_goal_phase = operator_run_active_goal_phase(private_events);
	let public_progress_phase = operator_run_public_progress_phase(private_events);
	let phase_acceptance = operator_run_phase_acceptance_status(private_events);
	let loop_status = operator_run_loop_status(
		project,
		loop_evidence,
		&run,
		&lifecycle.status,
		&lifecycle.phase,
		&lifecycle.current_operation,
	)?;

	Ok(hydrate_operator_run_derived_status(status::operator_run_status_from_parts(
		OperatorRunStatusParts {
			project,
			project_display_name,
			run: &run,
			lifecycle,
			wait_reason,
			app_server_state,
			timing,
			protocol_summary,
			child_agent_activity,
			protocol_activity,
			progress_diagnostic,
			account,
			accounts,
			branch_name,
			worktree_path,
			issue_identifier,
			private_evidence,
			continuation_recovery,
			phase_acceptance,
			active_goal_phase,
			public_progress_phase,
			loop_status,
		},
	)))
}

pub(crate) fn operator_run_active_goal_phase(events: &[PrivateExecutionEvent]) -> Option<String> {
	phase::operator_run_active_goal_phase(events)
}

pub(crate) fn operator_run_public_progress_phase(
	events: &[PrivateExecutionEvent],
) -> Option<String> {
	phase::operator_run_public_progress_phase(events)
}

pub(crate) fn operator_run_phase_acceptance_status(
	events: &[PrivateExecutionEvent],
) -> Option<OperatorPhaseAcceptanceStatus> {
	phase::operator_run_phase_acceptance_status(events)
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
