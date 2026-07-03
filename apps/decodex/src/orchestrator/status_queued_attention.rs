//! Queued issue attention projection for operator status snapshots.

mod active_label;
mod records;

pub(crate) use self::records::operator_authority_decision_request_status_from_event;

use std::path::PathBuf;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		self, ATTENTION_ERROR_EVIDENCE_MISSING, AUTHORITY_DECISION_REQUEST_EVENT_TYPE,
		OperatorAuthorityDecisionRequestStatus, OperatorLoopStatus,
		OperatorQueuedIssueAttentionStatus, QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT,
		WorktreeTrackedChangeState, marker_process_liveness_for_marker,
		status_run_projection::{self},
	},
	prelude::Result,
	state::{
		self, RUN_OPERATION_AGENT_RUN, RUN_OPERATION_APP_SERVER_PREFLIGHT,
		RUN_OPERATION_GIT_CREDENTIALS, RUN_OPERATION_RECONCILIATION, RunActivityMarker, StateStore,
	},
	tracker::{IssueTracker, TrackerIssue, records::LinearExecutionEventRecord},
	workflow::WorkflowDocument,
};

struct OperatorQueuedIssueWorktreeContext {
	path: PathBuf,
	marker: Option<RunActivityMarker>,
	marker_unreadable: bool,
}

pub(super) fn operator_queued_issue_attention_status<T>(
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
		operator_queued_issue_worktree_context(project, state_store, issue)?;
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
	let summary = operator_queued_issue_attention_summary(
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

fn operator_queued_issue_worktree_context(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Result<OperatorQueuedIssueWorktreeContext> {
	let worktree_mapping = state_store.worktree_for_issue(&issue.id)?;
	let path = worktree_mapping
		.as_ref()
		.map(|mapping| mapping.worktree_path().to_path_buf())
		.unwrap_or_else(|| project.worktree_root().join(&issue.identifier));
	let marker = state::read_run_activity_marker_snapshot(&path).unwrap_or_default();
	let marker_unreadable = marker.is_none()
		&& matches!(path.join(state::RUN_ACTIVITY_MARKER_FILE).try_exists(), Ok(true));

	Ok(OperatorQueuedIssueWorktreeContext { path, marker, marker_unreadable })
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
		.and_then(operator_authority_decision_request_status_from_event))
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

fn operator_queued_issue_attention_summary(
	reason: &str,
	marker: Option<&RunActivityMarker>,
	attempt_status: Option<&str>,
	retry_budget_attempts: i64,
	worktree_tracked_change_state: WorktreeTrackedChangeState,
	attention_error_class: Option<&str>,
) -> String {
	if let Some(summary) = active_label::operator_active_label_attention_summary(
		reason,
		marker,
		retry_budget_attempts,
		worktree_tracked_change_state,
		attention_error_class,
	) {
		return summary;
	}

	if attempt_status == Some("failed")
		&& marker.and_then(RunActivityMarker::last_event_type).is_some_and(|event_type| {
			matches!(event_type, "thread/archive" | "thread/archive/discarded")
		}) {
		let operation = operator_recovery_operation_label(marker);

		return format!(
			"Child implementation attempt failed during {operation}; retained status is preserved separately from parent journal or closeout handling."
		);
	}
	if worktree_tracked_change_state.has_tracked_changes() {
		if retry_budget_attempts > 0 {
			return format!(
				"Partial worktree changes are retained after {retry_budget_attempts} failed attempts; inspect the patch, finish validation, then land or reset manually."
			);
		}
		if attention_error_class == Some("partial_progress_retained") {
			return String::from(
				"Partial worktree changes are retained after a stalled or failed attempt; inspect the patch, finish validation, then land or reset manually.",
			);
		}
	}
	if attention_error_class == Some("app_server_plugin_list_timeout") {
		return String::from(
			"app_server_preflight_failed: plugin/list timed out during Codex app-server preflight; operator recovery required.",
		);
	}
	if marker
		.and_then(RunActivityMarker::thread_status)
		.is_some_and(|status| status == "systemError")
	{
		return if retry_budget_attempts > 0 {
			format!(
				"App-server thread ended with systemError after {retry_budget_attempts} retry-budget attempts."
			)
		} else {
			String::from("App-server thread ended with systemError.")
		};
	}
	if reason == "retry_budget_exhausted" {
		return if retry_budget_attempts > 0 {
			format!(
				"Retry budget has {retry_budget_attempts} recorded failed attempts; operator recovery required."
			)
		} else {
			String::from("Retry budget exhausted; operator recovery required.")
		};
	}

	if let Some(status) = attempt_status {
		let operation = operator_recovery_operation_label(marker);

		match status {
			"interrupted" => {
				return format!(
					"Previous attempt was interrupted during {operation}; operator recovery required."
				);
			},
			"stalled" => {
				return format!(
					"Previous attempt stalled during {operation}; operator recovery required."
				);
			},
			"failed" => {
				return format!(
					"Child implementation attempt failed during {operation}; retained status is preserved separately from parent journal or closeout handling."
				);
			},
			"terminal_guarded" => {
				return format!(
					"Previous attempt hit a terminal guard during {operation}; operator recovery required."
				);
			},
			_ => {},
		}
	}

	if marker
		.and_then(RunActivityMarker::last_event_type)
		.is_some_and(|event_type| event_type == "item/tool/call")
	{
		return String::from("Stopped during a tool call; operator recovery required.");
	}

	match marker.and_then(RunActivityMarker::current_operation) {
		Some(RUN_OPERATION_GIT_CREDENTIALS) =>
			String::from("Git credential preflight failed; operator recovery required."),
		Some(RUN_OPERATION_APP_SERVER_PREFLIGHT) =>
			String::from("Codex app-server preflight failed; operator recovery required."),
		Some(RUN_OPERATION_RECONCILIATION) => String::from(
			"Stopped during reconciliation or tracker handoff; operator recovery required.",
		),
		Some(RUN_OPERATION_AGENT_RUN) =>
			String::from("Stopped during agent execution; operator recovery required."),
		Some(operation) => format!("Stopped during `{operation}`; operator recovery required."),
		None => String::from("Needs operator recovery; no local run marker was found."),
	}
}

fn operator_recovery_operation_label(marker: Option<&RunActivityMarker>) -> String {
	match marker.and_then(RunActivityMarker::current_operation) {
		Some(RUN_OPERATION_GIT_CREDENTIALS) => String::from("git credential preflight"),
		Some(RUN_OPERATION_APP_SERVER_PREFLIGHT) => String::from("Codex app-server preflight"),
		Some(RUN_OPERATION_RECONCILIATION) => String::from("reconciliation or tracker handoff"),
		Some(RUN_OPERATION_AGENT_RUN) => String::from("agent execution"),
		Some(operation) => format!("`{operation}`"),
		None => String::from("the lane"),
	}
}
