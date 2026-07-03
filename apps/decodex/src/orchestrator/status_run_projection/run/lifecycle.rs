use crate::orchestrator::{
	OperatorRunAppServerState, OperatorRunLifecycleProjection, OperatorRunProtocolSummary,
	OperatorRunTiming, OperatorTerminalFinalizeProjection, ProjectRunStatus, RunActivityMarker,
	status_process_liveness, status_run_projection::runtime,
};

pub(super) fn operator_run_lifecycle_projection(
	run: &ProjectRunStatus,
	marker: Option<&RunActivityMarker>,
	terminal_finalize_projection: Option<OperatorTerminalFinalizeProjection>,
	timing: &OperatorRunTiming,
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
	now_unix_epoch: i64,
) -> OperatorRunLifecycleProjection {
	let marker_current_operation = marker.and_then(RunActivityMarker::current_operation);
	let status = terminal_finalize_projection
		.map(|projection| projection.status.to_owned())
		.unwrap_or_else(|| {
			runtime::operator_run_visible_status(
				run.status(),
				app_server_state,
				protocol_summary,
				timing,
				marker_current_operation,
			)
		});
	let status_projection_reason = if terminal_finalize_projection.is_some() {
		None
	} else {
		runtime::operator_run_status_projection_reason(
			run.status(),
			&status,
			app_server_state,
			protocol_summary,
			timing,
			marker_current_operation,
		)
	};
	let (retry_kind, retry_ready_at_unix_epoch) = runtime::visible_operator_run_retry_schedule(
		&status,
		marker.and_then(RunActivityMarker::retry_kind),
		marker.and_then(RunActivityMarker::retry_ready_at_unix_epoch),
		now_unix_epoch,
	);
	let (phase, wait_reason) = if let Some(projection) = terminal_finalize_projection {
		(String::from(projection.phase), Some(String::from(projection.wait_reason)))
	} else {
		runtime::classify_operator_run_phase(
			&status,
			retry_kind.as_deref(),
			retry_ready_at_unix_epoch,
			now_unix_epoch,
		)
	};
	let current_operation = terminal_finalize_projection
		.map(|projection| projection.current_operation.to_owned())
		.unwrap_or_else(|| {
			runtime::classify_operator_run_operation(&phase, marker_current_operation)
		});
	let suspected_stall = terminal_finalize_projection.is_none()
		&& runtime::operator_run_is_suspected_stall(
			&phase,
			timing.last_progress_unix_epoch,
			now_unix_epoch,
			status_process_liveness::run_activity_idle_timeout(marker),
		);
	let execution_liveness = if terminal_finalize_projection.is_some() {
		String::from("not_running")
	} else {
		runtime::operator_run_execution_liveness(
			&status,
			timing,
			app_server_state,
			protocol_summary,
		)
	};
	let run_lease = terminal_finalize_projection.is_none() && run.run_lease();

	OperatorRunLifecycleProjection {
		status,
		status_projection_reason,
		phase,
		wait_reason,
		current_operation,
		suspected_stall,
		execution_liveness,
		run_lease,
		retry_kind,
		retry_ready_at_unix_epoch,
	}
}
