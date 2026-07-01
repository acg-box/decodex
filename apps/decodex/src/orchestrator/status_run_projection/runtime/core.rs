#[allow(clippy::wildcard_imports)] use super::*;

pub(in crate::orchestrator) fn operator_run_control_capability(
	run: &ProjectRunStatus,
	app_server_state: &OperatorRunAppServerState,
) -> Option<OperatorRunControlCapability> {
	let channel = run.control_channel()?;

	Some(OperatorRunControlCapability {
		project_id: channel.project_id().to_owned(),
		issue_id: channel.issue_id().to_owned(),
		run_id: channel.run_id().to_owned(),
		attempt_number: channel.attempt_number(),
		thread_id: app_server_state.thread_id.clone(),
		turn_id: app_server_state.turn_id.clone(),
		transport: channel.transport().to_owned(),
		channel_path: channel.channel_path().display().to_string(),
		status: channel.status().to_owned(),
		published_at: channel.published_at().to_owned(),
		updated_at: channel.updated_at().to_owned(),
	})
}

pub(in crate::orchestrator) fn load_operator_run_marker(
	run: &ProjectRunStatus,
) -> crate::prelude::Result<Option<RunActivityMarker>> {
	let marker = run.worktree_path().and_then(|worktree_path| {
		state::read_run_activity_marker_snapshot(worktree_path).unwrap_or_default()
	});

	Ok(marker.filter(|marker| {
		marker.run_id() == run.run_id() && marker.attempt_number() == run.attempt_number()
	}))
}

pub(in crate::orchestrator) fn operator_run_timing(
	run: &ProjectRunStatus,
	marker: Option<&RunActivityMarker>,
	now_unix_epoch: i64,
) -> OperatorRunTiming {
	let process_id = marker.and_then(RunActivityMarker::process_id);
	let last_run_activity_unix_epoch = max_optional_i64(
		Some(run.last_run_activity_unix_epoch()),
		marker.and_then(RunActivityMarker::last_activity_unix_epoch),
	);
	let last_protocol_activity_unix_epoch = max_optional_i64(
		run.last_event_at_unix(),
		marker.and_then(RunActivityMarker::last_protocol_activity_unix_epoch),
	);
	let run_event_progress_unix_epoch = run
		.last_event_type()
		.filter(|event_type| state::protocol_event_counts_as_work_progress(event_type))
		.and_then(|_| run.last_event_at_unix());
	let last_progress_unix_epoch = max_optional_i64(
		marker.and_then(RunActivityMarker::last_progress_unix_epoch),
		run_event_progress_unix_epoch,
	);
	let process_liveness = marker.and_then(marker_process_liveness_for_marker);

	OperatorRunTiming {
		process_alive: process_liveness.map(|liveness| liveness.alive),
		process_liveness_reason: process_liveness.map(|liveness| liveness.reason.to_owned()),
		process_id,
		last_run_activity_unix_epoch,
		last_protocol_activity_unix_epoch,
		last_progress_unix_epoch,
		idle_for_seconds: idle_duration_seconds(last_run_activity_unix_epoch, now_unix_epoch),
		protocol_idle_for_seconds: idle_duration_seconds(
			last_protocol_activity_unix_epoch,
			now_unix_epoch,
		),
	}
}

pub(in crate::orchestrator) fn operator_run_app_server_state(
	run: &ProjectRunStatus,
	marker: Option<&RunActivityMarker>,
) -> OperatorRunAppServerState {
	let thread_active_flags =
		marker.map(|marker| marker.thread_active_flags().to_vec()).unwrap_or_default();

	OperatorRunAppServerState {
		thread_id: run
			.thread_id()
			.or_else(|| marker.and_then(RunActivityMarker::thread_id))
			.map(str::to_owned),
		turn_id: run
			.turn_id()
			.or_else(|| marker.and_then(RunActivityMarker::turn_id))
			.map(str::to_owned),
		thread_status: marker.and_then(RunActivityMarker::thread_status).map(str::to_owned),
		interactive_requested: thread_active_flags
			.iter()
			.any(|flag| matches!(flag.as_str(), "waitingOnApproval" | "waitingOnUserInput")),
		continuation_pending: run.status() == CONTINUATION_PENDING_RUN_STATUS,
		effective_model: marker.and_then(RunActivityMarker::effective_model).map(str::to_owned),
		effective_model_provider: marker
			.and_then(RunActivityMarker::effective_model_provider)
			.map(str::to_owned),
		effective_cwd: marker.and_then(RunActivityMarker::effective_cwd).map(str::to_owned),
		effective_approval_policy: marker
			.and_then(RunActivityMarker::effective_approval_policy)
			.map(str::to_owned),
		effective_approvals_reviewer: marker
			.and_then(RunActivityMarker::effective_approvals_reviewer)
			.map(str::to_owned),
		effective_sandbox_mode: marker
			.and_then(RunActivityMarker::effective_sandbox_mode)
			.map(str::to_owned),
		thread_active_flags,
	}
}

pub(in crate::orchestrator) fn operator_run_protocol_summary(
	run: &ProjectRunStatus,
	marker: Option<&RunActivityMarker>,
) -> OperatorRunProtocolSummary {
	let use_marker_protocol_summary =
		run.event_count() == 0 && run.last_event_type().is_none() && run.last_event_at().is_none()
			|| marker_protocol_summary_supersedes_run(run, marker);

	if use_marker_protocol_summary {
		return OperatorRunProtocolSummary {
			last_event_type: marker.and_then(RunActivityMarker::last_event_type).map(str::to_owned),
			last_event_at: marker
				.and_then(RunActivityMarker::last_protocol_activity_unix_epoch)
				.and_then(|unix_epoch| format_optional_unix_timestamp(Some(unix_epoch))),
			event_count: marker.map_or(0, RunActivityMarker::event_count),
		};
	}

	OperatorRunProtocolSummary {
		last_event_type: run.last_event_type().map(str::to_owned),
		last_event_at: run.last_event_at().map(str::to_owned),
		event_count: run.event_count(),
	}
}

pub(in crate::orchestrator) fn marker_protocol_summary_supersedes_run(
	run: &ProjectRunStatus,
	marker: Option<&RunActivityMarker>,
) -> bool {
	let Some(marker) = marker else {
		return false;
	};

	if marker.last_event_type().is_none() {
		return false;
	}

	let Some(marker_event_at) = marker.last_protocol_activity_unix_epoch() else {
		return false;
	};

	run.last_event_at_unix().is_none_or(|run_event_at| {
		marker_event_at > run_event_at
			|| marker_event_at == run_event_at && marker.event_count() > run.event_count()
	})
}
