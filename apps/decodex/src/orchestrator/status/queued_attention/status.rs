mod readback;
mod retry;
mod timing;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		self, ATTENTION_ERROR_EVIDENCE_MISSING, OperatorQueuedIssueAttentionStatus,
		QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT, WorktreeTrackedChangeState,
		marker_process_liveness_for_marker,
		status_queued_attention::{
			active_label,
			context::{self, OperatorQueuedIssueWorktreeContext},
			records, summary,
		},
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
	let retry_budget_attempts = retry::operator_queued_issue_retry_budget_attempts(
		state_store,
		project.service_id(),
		issue,
		marker.as_ref(),
	)?;
	let retry_budget_max_attempts = i64::from(workflow.frontmatter().execution().max_attempts());
	let auto_retry_blocked_reason = retry::operator_queued_issue_auto_retry_blocked_reason(reason);
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
	let attention_error_class = operator_queued_issue_attention_error_class(
		private_evidence_missing,
		attention_record.as_ref(),
	);
	let decision_request = readback::operator_queued_issue_decision_request_status(
		project,
		state_store,
		issue,
		attention_record.as_ref(),
		marker.as_ref(),
	)?;
	let loop_status = readback::operator_queued_issue_loop_status(
		project,
		state_store,
		issue,
		attention_record.as_ref(),
		marker.as_ref(),
	)?;
	let attempt_status =
		readback::operator_queued_issue_attempt_status(state_store, marker.as_ref())?;
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
		last_activity_at: timing::operator_queued_issue_marker_activity_at(marker.as_ref()),
		last_progress_at: timing::operator_queued_issue_marker_progress_at(marker.as_ref()),
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

fn operator_queued_issue_attention_error_class(
	private_evidence_missing: bool,
	attention_record: Option<&LinearExecutionEventRecord>,
) -> Option<String> {
	if private_evidence_missing {
		Some(String::from(ATTENTION_ERROR_EVIDENCE_MISSING))
	} else {
		attention_record.and_then(|record| record.error_class.clone())
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
