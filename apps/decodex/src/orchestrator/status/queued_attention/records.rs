use serde_json::Value;

use crate::{
	config::ServiceConfig,
	orchestrator::{self, OperatorAuthorityDecisionRequestStatus, OperatorHistoryLedgerRecord},
	state::{PrivateExecutionEvent, StateStore},
	tracker::{IssueTracker, TrackerIssue, records::LinearExecutionEventRecord},
};

pub(crate) fn operator_authority_decision_request_status_from_event(
	event: &PrivateExecutionEvent,
) -> Option<OperatorAuthorityDecisionRequestStatus> {
	let payload = event.payload();
	let decision_request_id = payload.get("decision_request_id")?.as_str()?.to_owned();
	let reason = payload.get("reason")?.as_str()?.to_owned();
	let boundary = payload.get("boundary")?.as_str()?.to_owned();
	let phase = payload.get("phase").and_then(Value::as_str).unwrap_or("human_required").to_owned();
	let next_action = payload
		.get("next_action")
		.or_else(|| payload.get("resume_condition"))?
		.as_str()?
		.to_owned();
	let recommendation = payload.get("recommendation").and_then(Value::as_str).map(str::to_owned);
	let resume_condition =
		payload.get("resume_condition").and_then(Value::as_str).map(str::to_owned);

	Some(OperatorAuthorityDecisionRequestStatus {
		phase,
		reason,
		boundary,
		decision_request_id,
		next_action,
		recommendation,
		resume_condition,
	})
}

pub(crate) fn operator_queued_issue_latest_attention_record<T>(
	tracker: &T,
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Option<LinearExecutionEventRecord>
where
	T: IssueTracker,
{
	let local_records = state_store
		.list_linear_execution_events(project.service_id(), &issue.id)
		.inspect_err(|error| {
			tracing::debug!(
				?error,
				issue_id = issue.id,
				issue = issue.identifier,
				"Failed to load local attention records for queued issue."
			);
		})
		.ok();

	if let Some(record) =
		local_records.as_deref().and_then(latest_attention_record_from_linear_records)
	{
		return Some(record.clone());
	}

	let comments = tracker
		.list_comments(&issue.id)
		.inspect_err(|error| {
			tracing::debug!(
				?error,
				issue_id = issue.id,
				issue = issue.identifier,
				"Failed to load tracker comments for queued attention issue."
			);
		})
		.ok()?;
	let records =
		orchestrator::collect_history_ledger_records(project.service_id(), &issue.id, &comments);

	latest_attention_record_from_history_ledger_records(&records)
		.map(|record| record.record.clone())
}

fn latest_attention_record_from_linear_records(
	records: &[LinearExecutionEventRecord],
) -> Option<&LinearExecutionEventRecord> {
	records
		.iter()
		.filter(|record| {
			matches!(record.event_type.as_str(), "needs_attention" | "terminal_failure")
		})
		.max_by(|left, right| {
			orchestrator::parse_rfc3339_unix_epoch(&left.event_timestamp)
				.cmp(&orchestrator::parse_rfc3339_unix_epoch(&right.event_timestamp))
				.then_with(|| left.idempotency_key.cmp(&right.idempotency_key))
		})
}

fn latest_attention_record_from_history_ledger_records(
	records: &[OperatorHistoryLedgerRecord],
) -> Option<&OperatorHistoryLedgerRecord> {
	records
		.iter()
		.filter(|entry| {
			matches!(entry.record.event_type.as_str(), "needs_attention" | "terminal_failure")
		})
		.max_by(|left, right| orchestrator::compare_history_ledger_record_position(left, right))
}
