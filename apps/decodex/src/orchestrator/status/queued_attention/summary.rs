use crate::{
	orchestrator::{WorktreeTrackedChangeState, status_queued_attention::active_label},
	state::{
		RUN_OPERATION_AGENT_RUN, RUN_OPERATION_APP_SERVER_PREFLIGHT, RUN_OPERATION_GIT_CREDENTIALS,
		RUN_OPERATION_RECONCILIATION, RunActivityMarker,
	},
};

pub(crate) fn operator_queued_issue_attention_summary(
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
