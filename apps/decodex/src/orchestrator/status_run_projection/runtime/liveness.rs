#[allow(clippy::wildcard_imports)] use super::*;

pub(in crate::orchestrator) fn operator_run_visible_status(
	attempt_status: &str,
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
	_marker_current_operation: Option<&str>,
) -> String {
	if attempt_status == "starting"
		&& operator_run_has_app_server_execution_evidence(
			app_server_state,
			protocol_summary,
			timing,
		) {
		return String::from("running");
	}

	attempt_status.to_owned()
}

pub(in crate::orchestrator) fn operator_run_status_projection_reason(
	attempt_status: &str,
	visible_status: &str,
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
	_marker_current_operation: Option<&str>,
) -> Option<String> {
	if attempt_status == visible_status || visible_status != "running" {
		return None;
	}

	let projection_kind = if attempt_status == "starting" {
		"starting_attempt"
	} else {
		return None;
	};

	operator_run_live_evidence_source(app_server_state, protocol_summary, timing)
		.map(|source| format!("{projection_kind}_promoted_by_{source}"))
}

pub(in crate::orchestrator) fn operator_run_live_evidence_source(
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
) -> Option<&'static str> {
	if timing.process_alive == Some(true) {
		return Some("process_alive");
	}
	if matches!(app_server_state.thread_status.as_deref(), Some("active")) {
		return Some("thread_active");
	}
	if !app_server_state.thread_active_flags.is_empty() {
		return Some("thread_active_flags");
	}
	if operator_run_has_recent_protocol_execution_evidence(protocol_summary, timing) {
		return Some("recent_protocol_activity");
	}
	if app_server_state.effective_model.is_some()
		|| app_server_state.effective_model_provider.is_some()
		|| protocol_summary.event_count > 0
		|| protocol_summary.last_event_type.is_some()
	{
		return Some("app_server_metadata");
	}
	if timing.protocol_idle_for_seconds.is_some() {
		return Some("protocol_timing");
	}

	None
}

pub(in crate::orchestrator) fn operator_run_has_recent_protocol_execution_evidence(
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
) -> bool {
	operator_protocol_event_counts_as_live_execution(protocol_summary.last_event_type.as_deref())
		&& timing.protocol_idle_for_seconds.is_some_and(|idle_for| {
			u64::try_from(idle_for)
				.is_ok_and(|idle_for| idle_for < RUN_LEASE_IDLE_TIMEOUT.as_secs())
		})
}

pub(in crate::orchestrator) fn operator_protocol_event_counts_as_live_execution(
	event_type: Option<&str>,
) -> bool {
	let Some(event_type) = event_type else {
		return false;
	};

	state::protocol_event_counts_as_work_progress(event_type)
		&& !matches!(event_type.to_ascii_lowercase().as_str(), "thread/archive" | "turn/completed")
}

pub(in crate::orchestrator) fn operator_run_has_app_server_execution_evidence(
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
) -> bool {
	matches!(app_server_state.thread_status.as_deref(), Some("active"))
		|| !app_server_state.thread_active_flags.is_empty()
		|| app_server_state.effective_model.is_some()
		|| app_server_state.effective_model_provider.is_some()
		|| protocol_summary.event_count > 0
		|| protocol_summary.last_event_type.is_some()
		|| timing.protocol_idle_for_seconds.is_some_and(|idle_for| {
			u64::try_from(idle_for)
				.is_ok_and(|idle_for| idle_for < RUN_LEASE_IDLE_TIMEOUT.as_secs())
		})
}

pub(in crate::orchestrator) fn operator_run_queue_lease_state(run_lease: bool) -> String {
	if run_lease { String::from("held") } else { String::from("not_held") }
}

pub(in crate::orchestrator) fn operator_run_execution_liveness(
	status: &str,
	timing: &OperatorRunTiming,
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
) -> String {
	if !matches!(status, "starting" | "running") {
		return String::from("not_running");
	}
	if timing.process_alive == Some(true) {
		return String::from("process_alive");
	}
	if timing.process_alive == Some(false) {
		if process_liveness_reason_is_identity_mismatch(timing.process_liveness_reason.as_deref()) {
			return String::from(EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH);
		}

		return String::from("process_stopped");
	}
	if matches!(app_server_state.thread_status.as_deref(), Some("active"))
		|| !app_server_state.thread_active_flags.is_empty()
	{
		return String::from("thread_active");
	}
	if operator_run_has_app_server_execution_evidence(app_server_state, protocol_summary, timing) {
		return String::from("protocol_observed");
	}

	String::from("not_captured")
}

pub(in crate::orchestrator) fn process_liveness_reason_is_identity_mismatch(
	reason: Option<&str>,
) -> bool {
	matches!(reason, Some("host_boot_id_mismatch" | "process_start_identity_mismatch"))
}
