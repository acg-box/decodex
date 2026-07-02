use crate::orchestrator::{
	self, AUTHORITY_DECISION_REQUEST_EVENT_TYPE, OperatorLoopStatus, ProjectLoopEvidenceSnapshot,
	ServiceConfig, StateStore, status_run_projection,
};
use crate::prelude::Result;

pub(in crate::orchestrator) fn operator_loop_status_for_run(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_review_phase: Option<&str>,
	lifecycle_summary: Option<String>,
) -> Result<OperatorLoopStatus> {
	let loop_evidence = state_store.project_loop_evidence_snapshot(project.service_id())?;

	operator_loop_status_for_run_with_evidence(
		project,
		&loop_evidence,
		issue_id,
		run_id,
		attempt_number,
		default_review_phase,
		lifecycle_summary,
	)
}

pub(in crate::orchestrator) fn operator_loop_status_for_run_with_evidence(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_review_phase: Option<&str>,
	lifecycle_summary: Option<String>,
) -> Result<OperatorLoopStatus> {
	let review_level = project.codex().review_level();
	let review = status_run_projection::operator_review_loop_status(
		review_level,
		loop_evidence,
		issue_id,
		run_id,
		attempt_number,
		default_review_phase,
	)?;
	let events = loop_evidence.private_events(issue_id, run_id, attempt_number);
	let architecture_recovery = events
		.iter()
		.rev()
		.find_map(status_run_projection::operator_architecture_recovery_status_from_event);
	let boundary =
		events.iter().rev().find_map(status_run_projection::operator_boundary_status_from_event);
	let decision_request = events
		.iter()
		.rev()
		.find(|event| event.event_type() == AUTHORITY_DECISION_REQUEST_EVENT_TYPE)
		.and_then(orchestrator::operator_authority_decision_request_status_from_event);
	let autonomy_objective =
		orchestrator::operator_autonomy_objective_status(project, loop_evidence);
	let autonomy_signals = orchestrator::operator_autonomy_signal_statuses(loop_evidence);
	let autonomy_proposals = orchestrator::operator_autonomy_proposal_statuses(loop_evidence);
	let autonomy_lineage = orchestrator::operator_autonomy_lineage_statuses(loop_evidence);
	let autonomy_report = orchestrator::operator_autonomy_report_status(
		autonomy_objective.as_ref(),
		&autonomy_signals,
		&autonomy_proposals,
		&autonomy_lineage,
	);
	let autonomy = status_run_projection::operator_loop_autonomy(
		boundary.as_ref(),
		architecture_recovery.as_ref(),
		decision_request.as_ref(),
	);
	let summary = status_run_projection::operator_loop_status_summary(
		review.as_ref(),
		architecture_recovery.as_ref(),
		boundary.as_ref(),
		decision_request.as_ref(),
		autonomy,
		lifecycle_summary.as_deref(),
	);
	let next_action = status_run_projection::operator_loop_status_next_action(
		review.as_ref(),
		architecture_recovery.as_ref(),
		boundary.as_ref(),
		decision_request.as_ref(),
	);

	Ok(OperatorLoopStatus {
		review_level: review_level.as_str().to_owned(),
		autonomy: autonomy.to_owned(),
		summary,
		next_action,
		autonomy_objective,
		autonomy_signals,
		autonomy_proposals,
		autonomy_lineage,
		autonomy_report,
		review,
		architecture_recovery,
		boundary,
		decision_request,
	})
}
