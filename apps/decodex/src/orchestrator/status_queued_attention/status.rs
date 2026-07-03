use crate::{
	config::ServiceConfig,
	orchestrator::{
		self, ATTENTION_ERROR_EVIDENCE_MISSING, AUTHORITY_DECISION_REQUEST_EVENT_TYPE,
		OperatorAuthorityDecisionRequestStatus, OperatorLoopStatus,
		OperatorQueuedIssueAttentionStatus, QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT,
		WorktreeTrackedChangeState, marker_process_liveness_for_marker,
		status_queued_attention::{
			active_label,
			context::{self, OperatorQueuedIssueWorktreeContext},
			records, summary,
		},
		status_run_projection,
	},
	prelude::Result,
	state::{RunActivityMarker, StateStore},
	tracker::{IssueTracker, TrackerIssue, records::LinearExecutionEventRecord},
	workflow::WorkflowDocument,
};

pub(crate) fn operator_queued_issue_attention_status<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
	reason: &str,
) -> Result<Option<OperatorQueuedIssueAttentionStatus>>
where
	T: IssueTracker,
{
	if !matches!(
		reason,
		"issue_needs_attention"
			| "retry_budget_exhausted"
			| QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT
	) {
		return Ok(None);
	}

	let OperatorQueuedIssueWorktreeContext { path: worktree_path, marker, marker_unreadable } =
		context::operator_queued_issue_worktree_context(project, state_store, issue)?;
	let retry_budget_attempts =
		operator_queued_issue_retry_budget_attempts(state_store, issue, marker.as_ref())?;
	let retry_budget_max_attempts = i64::from(workflow.frontmatter().execution().max_attempts());
	let auto_retry_blocked_reason = operator_queued_issue_auto_retry_blocked_reason(reason);
	let attention_record = records::operator_queued_issue_latest_attention_record(
		tracker,
		project,
		state_store,
		issue,
	);
	let private_evidence_missing = operator_queued_issue_private_evidence_missing(
		project,
		state_store,
		issue,
		marker.as_ref(),
		reason,
	)?;
	let attention_error_class = if private_evidence_missing {
		Some(String::from(ATTENTION_ERROR_EVIDENCE_MISSING))
	} else {
		attention_record.as_ref().and_then(|record| record.error_class.clone())
	};
	let decision_request = operator_queued_issue_decision_request_status(
		project,
		state_store,
		issue,
		attention_record.as_ref(),
		marker.as_ref(),
	)?;
	let loop_status = operator_queued_issue_loop_status(
		project,
		state_store,
		issue,
		attention_record.as_ref(),
		marker.as_ref(),
	)?;
	let attempt_status = operator_queued_issue_attempt_status(state_store, marker.as_ref())?;
	let worktree_tracked_change_state = if marker_unreadable {
		WorktreeTrackedChangeState::Unknown
	} else {
		orchestrator::worktree_tracked_change_state(&worktree_path)
	};
	let worktree_has_tracked_changes = worktree_tracked_change_state.has_tracked_changes();
	let recorded_next_action =
		attention_record.as_ref().and_then(|record| record.next_action.clone());
	let stale_active_next_action = active_label::operator_active_label_attention_next_action(
		reason,
		&issue.identifier,
		worktree_tracked_change_state,
		attention_error_class.as_deref(),
	);
	let attention_next_action = operator_queued_issue_attention_next_action(
		private_evidence_missing,
		recorded_next_action,
		stale_active_next_action,
	);
	let summary = summary::operator_queued_issue_attention_summary(
		reason,
		marker.as_ref(),
		attempt_status.as_deref(),
		retry_budget_attempts,
		worktree_tracked_change_state,
		attention_error_class.as_deref(),
	);
	let process_liveness = marker.as_ref().and_then(marker_process_liveness_for_marker);

	Ok(Some(OperatorQueuedIssueAttentionStatus {
		summary,
		decision_request,
		run_id: marker.as_ref().map(|marker| marker.run_id().to_owned()),
		attempt_number: marker.as_ref().map(RunActivityMarker::attempt_number),
		current_operation: marker
			.as_ref()
			.and_then(RunActivityMarker::current_operation)
			.map(str::to_owned),
		thread_status: marker
			.as_ref()
			.and_then(RunActivityMarker::thread_status)
			.map(str::to_owned),
		attempt_status,
		loop_status,
		auto_retry_blocked_reason,
		attention_error_class,
		attention_next_action,
		retry_budget_attempt_count: (retry_budget_attempts > 0).then_some(retry_budget_attempts),
		retry_budget_max_attempts,
		last_activity_at: operator_queued_issue_marker_activity_at(marker.as_ref()),
		last_progress_at: operator_queued_issue_marker_progress_at(marker.as_ref()),
		last_event_type: marker
			.as_ref()
			.and_then(RunActivityMarker::last_event_type)
			.map(str::to_owned),
		event_count: marker.as_ref().map_or(0, RunActivityMarker::event_count),
		process_alive: process_liveness.map(|liveness| liveness.alive),
		process_liveness_reason: process_liveness.map(|liveness| liveness.reason.to_owned()),
		worktree_path: matches!(worktree_path.try_exists(), Ok(true))
			.then(|| orchestrator::relative_worktree_path_for_path(project, &worktree_path)),
		worktree_has_tracked_changes,
	}))
}

fn operator_queued_issue_marker_activity_at(marker: Option<&RunActivityMarker>) -> Option<String> {
	marker.and_then(RunActivityMarker::last_activity_unix_epoch).and_then(|unix_epoch| {
		status_run_projection::format_optional_unix_timestamp(Some(unix_epoch))
	})
}

fn operator_queued_issue_marker_progress_at(marker: Option<&RunActivityMarker>) -> Option<String> {
	marker.and_then(RunActivityMarker::last_progress_unix_epoch).and_then(|unix_epoch| {
		status_run_projection::format_optional_unix_timestamp(Some(unix_epoch))
	})
}

fn operator_queued_issue_retry_budget_attempts(
	state_store: &StateStore,
	issue: &TrackerIssue,
	marker: Option<&RunActivityMarker>,
) -> Result<i64> {
	let state_retry_attempts = state_store.retry_budget_attempt_count(&issue.id)?;
	let marker_retry_attempts =
		marker.and_then(RunActivityMarker::retry_budget_attempt_count).unwrap_or(0);

	Ok(state_retry_attempts.max(marker_retry_attempts))
}

fn operator_queued_issue_auto_retry_blocked_reason(reason: &str) -> Option<String> {
	match reason {
		"issue_needs_attention" => Some(String::from("needs_attention_label")),
		QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT =>
			Some(String::from(QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT)),
		_ => None,
	}
}

fn operator_queued_issue_attention_next_action(
	private_evidence_missing: bool,
	recorded_next_action: Option<String>,
	stale_active_next_action: Option<String>,
) -> Option<String> {
	if private_evidence_missing || recorded_next_action.is_none() {
		stale_active_next_action.or(recorded_next_action)
	} else {
		recorded_next_action.or(stale_active_next_action)
	}
}

fn operator_queued_issue_attempt_status(
	state_store: &StateStore,
	marker: Option<&RunActivityMarker>,
) -> Result<Option<String>> {
	Ok(marker
		.and_then(|marker| state_store.run_attempt(marker.run_id()).transpose())
		.transpose()?
		.map(|run_attempt| run_attempt.status().to_owned()))
}

fn operator_queued_issue_loop_status(
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
		(Some(run_id), Some(attempt_number)) =>
			status_run_projection::operator_loop_status_for_run(
				project,
				state_store,
				&issue.id,
				run_id,
				attempt_number,
				Some("handoff"),
				None,
			)
			.map(Some),
		_ => Ok(None),
	}
}

fn operator_queued_issue_decision_request_status(
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

fn operator_queued_issue_private_evidence_missing(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
	marker: Option<&RunActivityMarker>,
	reason: &str,
) -> Result<bool> {
	if reason != QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT {
		return Ok(false);
	}

	let Some(marker) = marker else {
		return Ok(true);
	};
	let events = state_store.list_private_execution_events(
		project.service_id(),
		&issue.id,
		marker.run_id(),
		marker.attempt_number(),
	)?;

	Ok(events.is_empty())
}
