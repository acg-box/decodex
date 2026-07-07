use crate::{
	config::ServiceConfig,
	orchestrator::{
		AUTHORITY_DECISION_REQUEST_EVENT_TYPE, OperatorAuthorityDecisionRequestStatus,
		OperatorLoopStatus, status_queued_attention::records, status_run_projection,
	},
	prelude::Result,
	state::{RunActivityMarker, StateStore},
	tracker::{TrackerIssue, records::LinearExecutionEventRecord},
};

pub(crate) fn operator_queued_issue_attempt_status(
	state_store: &StateStore,
	marker: Option<&RunActivityMarker>,
) -> Result<Option<String>> {
	Ok(marker
		.and_then(|marker| state_store.run_attempt(marker.run_id()).transpose())
		.transpose()?
		.map(|run_attempt| run_attempt.status().to_owned()))
}

pub(crate) fn operator_queued_issue_loop_status(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
	attention_record: Option<&LinearExecutionEventRecord>,
	marker: Option<&RunActivityMarker>,
) -> Result<Option<OperatorLoopStatus>> {
	let run_id = attention_record
		.map(|record| record.run_id.as_str())
		.or_else(|| marker.map(RunActivityMarker::run_id));
	let attempt_number = attention_record
		.map(|record| record.attempt_number)
		.or_else(|| marker.map(RunActivityMarker::attempt_number));

	match (run_id, attempt_number) {
		(Some(run_id), Some(attempt_number)) => {
			status_run_projection::operator_loop_status_for_run(
				project,
				state_store,
				&issue.id,
				run_id,
				attempt_number,
				Some("handoff"),
				None,
			)
			.map(Some)
		},
		_ => Ok(None),
	}
}

pub(crate) fn operator_queued_issue_decision_request_status(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
	attention_record: Option<&LinearExecutionEventRecord>,
	marker: Option<&RunActivityMarker>,
) -> Result<Option<OperatorAuthorityDecisionRequestStatus>> {
	let run_id = attention_record
		.map(|record| record.run_id.as_str())
		.or_else(|| marker.map(RunActivityMarker::run_id));
	let attempt_number = attention_record
		.map(|record| record.attempt_number)
		.or_else(|| marker.map(RunActivityMarker::attempt_number));
	let (Some(run_id), Some(attempt_number)) = (run_id, attempt_number) else {
		return Ok(None);
	};
	let events = state_store.list_private_execution_events(
		project.service_id(),
		&issue.id,
		run_id,
		attempt_number,
	)?;

	Ok(events
		.iter()
		.rev()
		.find(|event| event.event_type() == AUTHORITY_DECISION_REQUEST_EVENT_TYPE)
		.and_then(records::operator_authority_decision_request_status_from_event))
}
