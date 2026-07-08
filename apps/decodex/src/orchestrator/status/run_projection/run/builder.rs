use crate::{
	orchestrator::{
		OperatorRunStatus, ProjectLoopEvidenceSnapshot, ProjectRunStatus, ServiceConfig,
		status_process_liveness,
		status_run_projection::{
			self,
			run::status::{self, OperatorRunStatusParts},
		},
	},
	prelude::Result,
};

pub(crate) fn operator_run_status(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	project_display_name: &str,
	run: ProjectRunStatus,
	now_unix_epoch: i64,
) -> Result<OperatorRunStatus> {
	let marker = status_run_projection::load_operator_run_marker(&run)?;
	let timing = status_run_projection::operator_run_timing(&run, marker.as_ref(), now_unix_epoch);
	let app_server_state =
		status_run_projection::operator_run_app_server_state(&run, marker.as_ref());
	let protocol_summary =
		status_run_projection::operator_run_protocol_summary(&run, marker.as_ref());
	let terminal_finalize_projection =
		status_run_projection::operator_run_terminal_finalize_projection(loop_evidence, &run);
	let lifecycle = status_run_projection::operator_run_lifecycle_projection(
		&run,
		marker.as_ref(),
		terminal_finalize_projection,
		&timing,
		&app_server_state,
		&protocol_summary,
		now_unix_epoch,
	);
	let child_agent_activity = status_run_projection::operator_run_child_agent_activity(
		marker.as_ref(),
		run.child_agent_activity(),
		now_unix_epoch,
	);
	let protocol_activity = status_run_projection::operator_run_protocol_activity(
		marker.as_ref(),
		run.protocol_activity(),
		&app_server_state,
		child_agent_activity.as_ref(),
		timing.protocol_idle_for_seconds,
		matches!(lifecycle.status.as_str(), "starting" | "running"),
	);
	let wait_reason = status_run_projection::operator_run_wait_reason(
		&lifecycle.phase,
		lifecycle.wait_reason.clone(),
		protocol_activity.as_ref(),
	);
	let private_events =
		loop_evidence.private_events(run.issue_id(), run.run_id(), run.attempt_number());
	let progress_diagnostic = status_run_projection::operator_run_progress_diagnostic(
		&lifecycle.phase,
		&timing,
		protocol_activity.as_ref(),
		private_events,
		now_unix_epoch,
		status_process_liveness::run_activity_idle_timeout(marker.as_ref()),
	);
	let (account, accounts) = status_run_projection::operator_run_accounts(marker.as_ref());
	let branch_name = run.branch_name().map(str::to_owned);
	let worktree_path = status_run_projection::operator_run_relative_worktree_path(project, &run);
	let issue_identifier = status_run_projection::operator_run_issue_identifier_from_fields(
		run.run_id(),
		branch_name.as_deref(),
		worktree_path.as_deref(),
	);
	let private_evidence = status_run_projection::operator_run_private_evidence(
		project,
		&run,
		issue_identifier.as_deref(),
	);
	let continuation_recovery =
		status_run_projection::operator_run_continuation_recovery_status(loop_evidence, &run);
	let active_goal_phase = status_run_projection::operator_run_active_goal_phase(private_events);
	let public_progress_phase =
		status_run_projection::operator_run_public_progress_phase(private_events);
	let validation_evidence =
		status_run_projection::operator_run_validation_evidence_status(private_events);
	let loop_status = status_run_projection::operator_run_loop_status(
		project,
		loop_evidence,
		&run,
		&lifecycle.status,
		&lifecycle.phase,
		&lifecycle.current_operation,
	)?;

	Ok(status_run_projection::hydrate_operator_run_derived_status(
		status::operator_run_status_from_parts(OperatorRunStatusParts {
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
			validation_evidence,
			active_goal_phase,
			public_progress_phase,
			loop_status,
		}),
	))
}
