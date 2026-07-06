use crate::orchestrator::{
	OperatorLaneLifecycleAttemptEvidence, OperatorRunStatus,
	status_run_projection::history::lifecycle::phase,
};

pub(crate) fn operator_lane_lifecycle_attempt_evidence(
	run: &OperatorRunStatus,
) -> OperatorLaneLifecycleAttemptEvidence {
	let phase = phase::operator_run_lifecycle_metric_phase(run);
	let child_event_count =
		run.child_agent_activity.as_ref().map(|summary| summary.event_count.max(0)).unwrap_or(0);

	OperatorLaneLifecycleAttemptEvidence {
		run_id: run.run_id.clone(),
		issue_id: run.issue_id.clone(),
		attempt_number: run.attempt_number,
		status: run.status.clone(),
		phase: phase.key.to_owned(),
		source: run.lifecycle_source.clone(),
		evidence: run.lifecycle_evidence.clone(),
		gaps: run.lifecycle_gaps.clone(),
		protocol_event_count: run.event_count.max(0),
		child_event_count,
		updated_at: run.updated_at.clone(),
	}
}
