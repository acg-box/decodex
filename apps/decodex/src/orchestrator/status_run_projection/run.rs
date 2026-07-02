mod accessors;
mod lane_control;
mod lifecycle;
mod phase;

use crate::{
	orchestrator::{
		AgentPrivateEvidenceRef, ChildAgentActivitySummary, CodexAccountActivitySummary,
		OperatorContinuationRecoveryStatus, OperatorLaneLifecycleMetrics, OperatorLoopStatus,
		OperatorPhaseAcceptanceStatus, OperatorRunAppServerState, OperatorRunLifecycleProjection,
		OperatorRunProtocolSummary, OperatorRunStatus, OperatorRunTiming,
		OperatorTerminalFinalizeProjection, PrivateExecutionEvent, ProjectLoopEvidenceSnapshot,
		ProjectRunStatus, ProtocolActivitySummary, RunActivityMarker, ServiceConfig,
		status_process_liveness,
	},
	prelude::Result,
};

pub(in crate::orchestrator) fn operator_run_status(
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

	Ok(hydrate_operator_run_derived_status(operator_run_status_from_parts(
		project,
		project_display_name,
		&run,
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
	)))
}

#[allow(clippy::too_many_arguments)]
pub(in crate::orchestrator) fn operator_run_status_from_parts(
	project: &ServiceConfig,
	project_display_name: &str,
	run: &ProjectRunStatus,
	lifecycle: OperatorRunLifecycleProjection,
	wait_reason: Option<String>,
	app_server_state: OperatorRunAppServerState,
	timing: OperatorRunTiming,
	protocol_summary: OperatorRunProtocolSummary,
	child_agent_activity: Option<ChildAgentActivitySummary>,
	protocol_activity: Option<ProtocolActivitySummary>,
	progress_diagnostic: Option<String>,
	account: Option<CodexAccountActivitySummary>,
	accounts: Vec<CodexAccountActivitySummary>,
	branch_name: Option<String>,
	worktree_path: Option<String>,
	issue_identifier: Option<String>,
	private_evidence: AgentPrivateEvidenceRef,
	continuation_recovery: Option<OperatorContinuationRecoveryStatus>,
	phase_acceptance: Option<OperatorPhaseAcceptanceStatus>,
	active_goal_phase: Option<String>,
	public_progress_phase: Option<String>,
	loop_status: OperatorLoopStatus,
) -> OperatorRunStatus {
	let run_phase = lifecycle.phase.clone();

	OperatorRunStatus {
		project_id: project.service_id().to_owned(),
		project_display_name: project_display_name.to_owned(),
		run_id: run.run_id().to_owned(),
		issue_id: run.issue_id().to_owned(),
		issue_identifier,
		title: None,
		author: None,
		issue_state: None,
		active_label_present: None,
		needs_attention_label_present: None,
		attempt_number: run.attempt_number(),
		status: lifecycle.status,
		attempt_status: run.status().to_owned(),
		status_projection_reason: lifecycle.status_projection_reason,
		ownership_state: String::new(),
		liveness_state: String::new(),
		policy_state: String::new(),
		terminalization_state: String::new(),
		lane_control_next_action: String::new(),
		lane_control_conditions: Vec::new(),
		phase: lifecycle.phase,
		run_phase,
		wait_reason,
		current_operation: lifecycle.current_operation,
		active_goal_phase,
		public_progress_phase,
		control_capability: super::operator_run_control_capability(run, &app_server_state),
		thread_id: app_server_state.thread_id,
		turn_id: app_server_state.turn_id,
		thread_status: app_server_state.thread_status,
		thread_active_flags: app_server_state.thread_active_flags,
		interactive_requested: app_server_state.interactive_requested,
		continuation_pending: app_server_state.continuation_pending,
		continuation_recovery,
		phase_acceptance,
		run_lease: lifecycle.run_lease,
		queue_lease_state: super::operator_run_queue_lease_state(lifecycle.run_lease),
		execution_liveness: lifecycle.execution_liveness,
		has_fresh_execution: false,
		counts_as_running: false,
		needs_attention: false,
		updated_at: run.updated_at().to_owned(),
		last_run_activity_at: super::format_optional_unix_timestamp(
			timing.last_run_activity_unix_epoch,
		),
		last_protocol_activity_at: super::format_optional_unix_timestamp(
			timing.last_protocol_activity_unix_epoch,
		),
		last_progress_at: super::format_optional_unix_timestamp(timing.last_progress_unix_epoch),
		idle_for_seconds: timing.idle_for_seconds,
		protocol_idle_for_seconds: timing.protocol_idle_for_seconds,
		suspected_stall: lifecycle.suspected_stall,
		progress_diagnostic,
		last_event_type: protocol_summary.last_event_type,
		last_event_at: protocol_summary.last_event_at,
		event_count: protocol_summary.event_count,
		private_evidence,
		loop_status: Some(loop_status),
		process_id: timing.process_id,
		process_alive: timing.process_alive,
		process_liveness_reason: timing.process_liveness_reason,
		retry_kind: lifecycle.retry_kind,
		next_retry_at: super::format_optional_unix_timestamp(lifecycle.retry_ready_at_unix_epoch),
		effective_model: app_server_state.effective_model,
		effective_model_provider: app_server_state.effective_model_provider,
		effective_cwd: app_server_state.effective_cwd,
		effective_approval_policy: app_server_state.effective_approval_policy,
		effective_approvals_reviewer: app_server_state.effective_approvals_reviewer,
		effective_sandbox_mode: app_server_state.effective_sandbox_mode,
		child_agent_activity,
		protocol_activity,
		lifecycle_source: run.recovery_source().to_owned(),
		lifecycle_evidence: run.recovery_evidence().to_vec(),
		lifecycle_gaps: run.recovery_gaps().to_vec(),
		lifecycle_metrics: OperatorLaneLifecycleMetrics::default(),
		account,
		accounts,
		branch_name,
		worktree_path,
	}
}

pub(in crate::orchestrator) fn operator_run_active_goal_phase(
	events: &[PrivateExecutionEvent],
) -> Option<String> {
	phase::operator_run_active_goal_phase(events)
}

pub(in crate::orchestrator) fn operator_run_public_progress_phase(
	events: &[PrivateExecutionEvent],
) -> Option<String> {
	phase::operator_run_public_progress_phase(events)
}

pub(in crate::orchestrator) fn operator_run_phase_acceptance_status(
	events: &[PrivateExecutionEvent],
) -> Option<OperatorPhaseAcceptanceStatus> {
	phase::operator_run_phase_acceptance_status(events)
}

pub(in crate::orchestrator) fn hydrate_operator_run_derived_status(
	status: OperatorRunStatus,
) -> OperatorRunStatus {
	lane_control::hydrate_operator_run_derived_status(status)
}

pub(in crate::orchestrator) fn operator_run_lane_control_readback(
	run: &OperatorRunStatus,
) -> lane_control::OperatorRunLaneControlReadback {
	lane_control::operator_run_lane_control_readback(run)
}

pub(in crate::orchestrator) fn operator_run_lifecycle_projection(
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

pub(in crate::orchestrator) fn operator_run_wait_reason(
	phase: &str,
	wait_reason: Option<String>,
	protocol_activity: Option<&ProtocolActivitySummary>,
) -> Option<String> {
	phase::operator_run_wait_reason(phase, wait_reason, protocol_activity)
}

pub(in crate::orchestrator) fn operator_run_accounts(
	marker: Option<&RunActivityMarker>,
) -> (Option<CodexAccountActivitySummary>, Vec<CodexAccountActivitySummary>) {
	accessors::operator_run_accounts(marker)
}

pub(in crate::orchestrator) fn operator_run_relative_worktree_path(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
) -> Option<String> {
	accessors::operator_run_relative_worktree_path(project, run)
}

pub(in crate::orchestrator) fn operator_run_private_evidence(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
) -> AgentPrivateEvidenceRef {
	accessors::operator_run_private_evidence(project, run, issue_identifier)
}

pub(in crate::orchestrator) fn operator_run_loop_status(
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

pub(in crate::orchestrator) fn operator_run_default_review_phase(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> Option<&'static str> {
	phase::operator_run_default_review_phase(status, phase, current_operation)
}

pub(in crate::orchestrator) fn operator_run_lifecycle_loop_summary(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> Option<String> {
	phase::operator_run_lifecycle_loop_summary(status, phase, current_operation)
}
