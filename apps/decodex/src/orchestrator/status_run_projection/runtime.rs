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

pub(in crate::orchestrator) fn operator_run_terminal_finalize_projection(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
) -> Option<OperatorTerminalFinalizeProjection> {
	let events = loop_evidence.private_events(run.issue_id(), run.run_id(), run.attempt_number());
	let path = events
		.iter()
		.rev()
		.find(|event| event.event_type() == "terminal_finalize")
		.and_then(|event| event.payload().get("path"))
		.and_then(Value::as_str)?;

	match path {
		"review_handoff" => Some(OperatorTerminalFinalizeProjection {
			status: "review_handoff_pending",
			phase: "terminal_pending",
			wait_reason: review_handoff_terminal_finalize_wait_reason(loop_evidence, run, events),
			current_operation: RUN_OPERATION_REVIEW_WRITEBACK,
		}),
		"review_repair" => Some(OperatorTerminalFinalizeProjection {
			status: "review_repair_pending",
			phase: "terminal_pending",
			wait_reason: review_repair_terminal_finalize_wait_reason(loop_evidence, run, events),
			current_operation: RUN_OPERATION_REVIEW_WRITEBACK,
		}),
		"closeout" => Some(OperatorTerminalFinalizeProjection {
			status: "closeout_pending",
			phase: "terminal_pending",
			wait_reason: "closeout_writeback",
			current_operation: RUN_OPERATION_REVIEW_WRITEBACK,
		}),
		"manual_attention" => Some(OperatorTerminalFinalizeProjection {
			status: "manual_attention_pending",
			phase: "terminal_pending",
			wait_reason: "manual_attention_writeback",
			current_operation: RUN_OPERATION_REVIEW_WRITEBACK,
		}),
		_ => None,
	}
}

pub(in crate::orchestrator) fn review_handoff_terminal_finalize_wait_reason(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
	events: &[PrivateExecutionEvent],
) -> &'static str {
	let Some(intent) = events.iter().rev().find(|event| {
		let payload = event.payload();

		event.event_type() == "review_completion_intent"
			&& payload.get("path").and_then(Value::as_str) == Some("review_handoff")
			&& payload.get("mode").and_then(Value::as_str) == Some("handoff")
			&& payload.get("pr_url").and_then(Value::as_str).is_some()
			&& payload.get("pr_head_oid").and_then(Value::as_str).is_some()
			&& payload.get("worktree_path").and_then(Value::as_str).is_some()
	}) else {
		return "review_handoff_writeback";
	};
	let Some(branch) = intent.payload().get("branch").and_then(Value::as_str) else {
		return "review_handoff_writeback";
	};

	if loop_evidence.review_lifecycle_record(run.issue_id(), branch).is_none() {
		return "review_handoff_writeback_missing_lifecycle_marker";
	}

	"review_handoff_writeback"
}

pub(in crate::orchestrator) fn review_repair_terminal_finalize_wait_reason(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
	events: &[PrivateExecutionEvent],
) -> &'static str {
	let Some(intent) = events.iter().rev().find(|event| {
		let payload = event.payload();

		event.event_type() == "review_completion_intent"
			&& payload.get("path").and_then(Value::as_str) == Some("review_repair")
			&& payload.get("mode").and_then(Value::as_str) == Some("repair")
			&& payload.get("pr_url").and_then(Value::as_str).is_some()
			&& payload.get("pr_head_ref").and_then(Value::as_str).is_some()
			&& payload.get("pr_head_oid").and_then(Value::as_str).is_some()
			&& payload.get("worktree_path").and_then(Value::as_str).is_some()
	}) else {
		return "review_repair_writeback";
	};
	let payload = intent.payload();
	let Some(branch) = payload.get("branch").and_then(Value::as_str) else {
		return "review_repair_writeback";
	};
	let Some(pr_url) = payload.get("pr_url").and_then(Value::as_str) else {
		return "review_repair_writeback";
	};
	let Some(pr_head_ref) = payload.get("pr_head_ref").and_then(Value::as_str) else {
		return "review_repair_writeback";
	};
	let Some(pr_head_oid) = payload.get("pr_head_oid").and_then(Value::as_str) else {
		return "review_repair_writeback";
	};
	let Some(lifecycle_record) = loop_evidence.review_lifecycle_record(run.issue_id(), branch)
	else {
		return "review_repair_writeback_missing_lifecycle_marker";
	};

	if lifecycle_record.pr_url() != pr_url
		|| lifecycle_record.pr_head_ref_name() != pr_head_ref
		|| lifecycle_record.pr_head_oid() != pr_head_oid
		|| lifecycle_record.head_sha() != pr_head_oid
	{
		return "review_repair_writeback_stale_lifecycle_marker";
	}

	"review_repair_writeback"
}

pub(in crate::orchestrator) fn operator_run_continuation_recovery_status(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
) -> Option<OperatorContinuationRecoveryStatus> {
	let recovery_events = loop_evidence
		.private_events_for_issue(run.issue_id())
		.into_iter()
		.filter(|event| event.attempt_number() <= run.attempt_number())
		.filter_map(operator_continuation_recovery_event_status)
		.collect::<Vec<_>>();
	let latest = recovery_events.last()?.clone();
	let recovery_count = recovery_events
		.iter()
		.filter(|event| {
			event.source_phase == latest.source_phase
				&& event.source_error_class == latest.source_error_class
				&& event.state == "continuation_scheduled"
		})
		.count() as i64;
	let budget_exceeded = latest.state == "continuation_blocked"
		|| recovery_count > PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT;

	Some(OperatorContinuationRecoveryStatus {
		state: latest.state,
		source_phase: latest.source_phase,
		next_phase: latest.next_phase,
		source_error_class: latest.source_error_class,
		source_error_message: latest.source_error_message,
		recorded_at: latest.recorded_at,
		run_id: latest.run_id,
		attempt_number: latest.attempt_number,
		recovery_count,
		automatic_continuation_limit: PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT,
		budget_exceeded,
		next_action: if budget_exceeded {
			String::from("stop_auto_continuation_and_request_architecture_recovery")
		} else {
			String::from("monitor_continuation_recovery")
		},
	})
}

pub(in crate::orchestrator) fn operator_continuation_recovery_event_status(
	event: &PrivateExecutionEvent,
) -> Option<OperatorContinuationRecoveryStatus> {
	let state = match event.event_type() {
		PHASE_GOAL_RECOVERY_EVENT_TYPE => "continuation_scheduled",
		PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE => "continuation_blocked",
		_ => return None,
	};
	let payload = event.payload();
	let event_payload = payload.get("payload").unwrap_or(payload);
	let source_phase = payload
		.get("phase")
		.and_then(Value::as_str)
		.or_else(|| event_payload.get("sourcePhase").and_then(Value::as_str))?
		.to_owned();
	let next_phase = event_payload.get("nextPhase")?.as_str()?.to_owned();
	let source_error_class = event_payload.get("sourceErrorClass")?.as_str()?.to_owned();
	let source_error_message =
		event_payload.get("sourceErrorMessage").and_then(Value::as_str).map(str::to_owned);

	Some(OperatorContinuationRecoveryStatus {
		state: String::from(state),
		source_phase,
		next_phase,
		source_error_class,
		source_error_message,
		recorded_at: event.recorded_at().to_owned(),
		run_id: event.run_id().to_owned(),
		attempt_number: event.attempt_number(),
		recovery_count: 0,
		automatic_continuation_limit: PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT,
		budget_exceeded: false,
		next_action: String::new(),
	})
}

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

pub(in crate::orchestrator) fn operator_run_child_agent_activity(
	marker: Option<&RunActivityMarker>,
	stored_summary: Option<&ChildAgentActivitySummary>,
	now_unix_epoch: i64,
) -> Option<ChildAgentActivitySummary> {
	if let Some(marker) = marker
		&& let Some(summary) = marker.child_agent_activity()
	{
		return Some(summary.clone().live_projection(now_unix_epoch));
	}

	stored_summary.cloned().map(ChildAgentActivitySummary::sealed_durable)
}

pub(in crate::orchestrator) fn operator_run_protocol_activity(
	marker: Option<&RunActivityMarker>,
	stored_summary: Option<&ProtocolActivitySummary>,
	app_server_state: &OperatorRunAppServerState,
	child_agent_activity: Option<&ChildAgentActivitySummary>,
	protocol_idle_for_seconds: Option<i64>,
	is_running: bool,
) -> Option<ProtocolActivitySummary> {
	let mut summary = marker
		.and_then(RunActivityMarker::protocol_activity)
		.or(stored_summary)
		.cloned()
		.unwrap_or_default();

	if is_running && summary.waiting_reason.is_none() && app_server_state.interactive_requested {
		summary.waiting_reason = Some(String::from("approval_or_user_input"));
	}
	if is_running
		&& summary.waiting_reason.is_none()
		&& let Some(child_agent_activity) = child_agent_activity
		&& let Some(current_bucket) = child_agent_activity.current_bucket.as_deref()
	{
		summary.waiting_reason = Some(protocol_wait_reason_from_child_bucket(current_bucket));
	}
	if is_running
		&& summary.waiting_reason.is_none()
		&& protocol_idle_for_seconds.is_some_and(|idle_for| {
			u64::try_from(idle_for)
				.is_ok_and(|idle_for| idle_for < RUN_LEASE_IDLE_TIMEOUT.as_secs())
		}) {
		summary.waiting_reason = Some(String::from("protocol_idleness"));
	}
	if summary.turn_status.is_none()
		&& summary.waiting_reason.is_none()
		&& summary.rate_limit_status.is_none()
		&& summary.recent_events.is_empty()
	{
		return None;
	}

	sanitize_operator_protocol_activity_summary(&mut summary);

	Some(summary)
}

pub(in crate::orchestrator) fn sanitize_operator_protocol_activity_summary(
	summary: &mut ProtocolActivitySummary,
) {
	for event in &mut summary.recent_events {
		if let Some(detail) = event.detail.as_deref()
			&& !operator_protocol_activity_detail_is_public(detail)
		{
			event.detail = Some(String::from("redacted_sensitive_detail"));
		}
	}
}

pub(in crate::orchestrator) fn operator_protocol_activity_detail_is_public(detail: &str) -> bool {
	public_text::validate_public_text_field("protocol_activity.detail", detail).is_ok()
		&& !contains_protocol_activity_host_path_shape(detail)
		&& !contains_protocol_activity_secret_shape(detail)
}

pub(in crate::orchestrator) fn contains_protocol_activity_host_path_shape(detail: &str) -> bool {
	let mut previous = None;
	let mut chars = detail.char_indices().peekable();

	while let Some((index, character)) = chars.next() {
		if character != '/' {
			previous = Some(character);

			continue;
		}
		if previous == Some(':') || previous == Some('/') {
			previous = Some(character);

			continue;
		}

		let path_boundary = index == 0
			|| previous.is_some_and(|previous| {
				previous.is_whitespace()
					|| matches!(previous, '"' | '\'' | '`' | '(' | '[' | '{' | '=')
			});
		let path_component = chars
			.peek()
			.map(|(_, next)| next.is_ascii_alphanumeric() || matches!(next, '.' | '_' | '-'))
			.unwrap_or(false);

		if path_boundary && path_component {
			return true;
		}

		previous = Some(character);
	}

	false
}

pub(in crate::orchestrator) fn contains_protocol_activity_secret_shape(detail: &str) -> bool {
	detail.split(protocol_activity_token_separator).any(|token| {
		let normalized = token.to_ascii_lowercase();

		normalized.starts_with("ghp_")
			|| normalized.starts_with("github_pat_")
			|| is_high_entropy_protocol_activity_token(token)
	})
}

pub(in crate::orchestrator) fn protocol_activity_token_separator(character: char) -> bool {
	!(character.is_ascii_alphanumeric() || character == '_' || character == '-')
}

pub(in crate::orchestrator) fn is_high_entropy_protocol_activity_token(token: &str) -> bool {
	if token.len() < 24 {
		return false;
	}

	let mut has_uppercase = false;
	let mut has_lowercase = false;
	let mut has_digit = false;
	let mut alphanumeric_count = 0;

	for character in token.chars() {
		if !character.is_ascii_alphanumeric() {
			continue;
		}

		alphanumeric_count += 1;
		has_uppercase |= character.is_ascii_uppercase();
		has_lowercase |= character.is_ascii_lowercase();
		has_digit |= character.is_ascii_digit();
	}

	alphanumeric_count >= 24 && has_uppercase && has_lowercase && has_digit
}

pub(in crate::orchestrator) fn protocol_wait_reason_from_child_bucket(
	current_bucket: &str,
) -> String {
	match current_bucket {
		"Model" => String::from("model_execution"),
		"Protocol" => String::from("protocol_activity"),
		_ => String::from("tool_execution"),
	}
}

pub(in crate::orchestrator) fn idle_duration_seconds(
	last_activity_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
) -> Option<i64> {
	last_activity_unix_epoch
		.and_then(|last_activity| now_unix_epoch.checked_sub(last_activity))
		.filter(|idle_for| *idle_for >= 0)
}

pub(in crate::orchestrator) fn max_optional_i64(
	left: Option<i64>,
	right: Option<i64>,
) -> Option<i64> {
	match (left, right) {
		(Some(left), Some(right)) => Some(left.max(right)),
		(Some(value), None) | (None, Some(value)) => Some(value),
		(None, None) => None,
	}
}

pub(in crate::orchestrator) fn format_optional_unix_timestamp(
	unix_epoch: Option<i64>,
) -> Option<String> {
	unix_epoch.and_then(|unix_epoch| {
		OffsetDateTime::from_unix_timestamp(unix_epoch)
			.ok()
			.and_then(|timestamp| timestamp.format(&Rfc3339).ok())
	})
}

pub(in crate::orchestrator) fn format_optional_i64(value: Option<i64>) -> String {
	value.map_or_else(|| String::from("none"), |value| value.to_string())
}

pub(in crate::orchestrator) fn classify_operator_run_operation(
	phase: &str,
	marker_current_operation: Option<&str>,
) -> String {
	match phase {
		"retry_backoff" | "waiting_continuation" => String::from(RUN_OPERATION_WAITING_EXTERNAL),
		"completed" | "failed" => String::from(RUN_OPERATION_IDLE),
		"stalled" => marker_current_operation
			.map(str::to_owned)
			.unwrap_or_else(|| String::from(RUN_OPERATION_IDLE)),
		"executing" => marker_current_operation
			.map(str::to_owned)
			.unwrap_or_else(|| String::from(RUN_OPERATION_AGENT_RUN)),
		_ => marker_current_operation
			.map(str::to_owned)
			.unwrap_or_else(|| String::from(RUN_OPERATION_IDLE)),
	}
}

pub(in crate::orchestrator) fn operator_run_is_suspected_stall(
	phase: &str,
	last_progress_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
	idle_timeout: Duration,
) -> bool {
	if phase != "executing" {
		return false;
	}

	last_progress_unix_epoch
		.and_then(|last_progress| observed_idle_duration(last_progress, now_unix_epoch))
		.is_some_and(|idle_for| {
			idle_for >= suspected_operator_run_stall_threshold(idle_timeout)
				&& idle_for < idle_timeout
		})
}

pub(in crate::orchestrator) fn suspected_operator_run_stall_threshold(
	idle_timeout: Duration,
) -> Duration {
	Duration::from_secs((idle_timeout.as_secs() / 2).max(1))
}

pub(in crate::orchestrator) fn operator_run_progress_diagnostic(
	phase: &str,
	timing: &OperatorRunTiming,
	protocol_activity: Option<&ProtocolActivitySummary>,
	private_events: &[PrivateExecutionEvent],
	now_unix_epoch: i64,
	idle_timeout: Duration,
) -> Option<String> {
	if let Some(repo_gate_diagnostic) =
		operator_latest_repo_gate_failure_progress_diagnostic(private_events)
	{
		return Some(repo_gate_diagnostic);
	}

	if phase != "executing" {
		return None;
	}

	let protocol_activity = protocol_activity?;

	if protocol_activity.waiting_reason.as_deref() != Some("model_execution")
		|| !protocol_activity_is_non_work_only(protocol_activity)
	{
		return None;
	}

	let protocol_idle = timing
		.last_protocol_activity_unix_epoch
		.and_then(|last_protocol| observed_idle_duration(last_protocol, now_unix_epoch))?;

	if protocol_idle >= idle_timeout {
		return None;
	}

	let progress_is_stale = timing
		.last_progress_unix_epoch
		.and_then(|last_progress| observed_idle_duration(last_progress, now_unix_epoch))
		.is_none_or(|idle_for| idle_for >= suspected_operator_run_stall_threshold(idle_timeout));

	progress_is_stale.then(|| String::from("protocol_only_activity"))
}

pub(in crate::orchestrator) fn operator_latest_repo_gate_failure_progress_diagnostic(
	private_events: &[PrivateExecutionEvent],
) -> Option<String> {
	private_events
		.iter()
		.rev()
		.find(|event| event.event_type() == "phase_goal_transition")
		.and_then(operator_repo_gate_failure_progress_diagnostic)
}

pub(in crate::orchestrator) fn operator_repo_gate_failure_progress_diagnostic(
	event: &PrivateExecutionEvent,
) -> Option<String> {
	if event.event_type() != "phase_goal_transition" {
		return None;
	}

	let transition_payload = event.payload().get("payload")?;
	let error_class = transition_payload.get("errorClass")?.as_str()?;

	if !error_class.starts_with("repo_gate_") {
		return None;
	}

	let failed_command = transition_payload
		.get("repoGateFailure")
		.and_then(|diagnostic| diagnostic.get("failed_command"))
		.and_then(Value::as_str)
		.unwrap_or("inspect_private_evidence");

	Some(format!("repo_gate_failure:{error_class}; failed_command:{failed_command}"))
}

pub(in crate::orchestrator) fn protocol_activity_is_non_work_only(
	protocol_activity: &ProtocolActivitySummary,
) -> bool {
	!protocol_activity.recent_events.is_empty()
		&& protocol_activity
			.recent_events
			.iter()
			.all(|event| !state::protocol_event_counts_as_work_progress(&event.event_type))
}

pub(in crate::orchestrator) fn visible_operator_run_retry_schedule(
	status: &str,
	retry_kind: Option<&str>,
	retry_ready_at_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
) -> (Option<String>, Option<i64>) {
	let Some(retry_ready_at_unix_epoch) = retry_ready_at_unix_epoch else {
		return (None, None);
	};

	if matches!(status, "starting" | "running") || retry_ready_at_unix_epoch <= now_unix_epoch {
		return (None, None);
	}

	(retry_kind.map(str::to_owned), Some(retry_ready_at_unix_epoch))
}

pub(in crate::orchestrator) fn classify_operator_run_phase(
	status: &str,
	retry_kind: Option<&str>,
	retry_ready_at_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
) -> (String, Option<String>) {
	if status == "stalled" {
		return (String::from("stalled"), Some(String::from("app_server_idle_timeout")));
	}

	if let Some(retry_ready_at_unix_epoch) = retry_ready_at_unix_epoch
		&& retry_ready_at_unix_epoch > now_unix_epoch
	{
		return (
			String::from("retry_backoff"),
			Some(match retry_kind {
				Some("continuation") => String::from("continuation_retry"),
				Some("failure") => String::from("failure_retry"),
				Some(other) => other.to_owned(),
				None => String::from("scheduled_retry"),
			}),
		);
	}

	match status {
		"starting" | "running" => (String::from("executing"), None),
		CONTINUATION_PENDING_RUN_STATUS =>
			(String::from("waiting_continuation"), Some(String::from("turn_boundary"))),
		"succeeded" => (String::from("completed"), None),
		"failed" | "interrupted" | TERMINAL_GUARDED_RUN_STATUS => (String::from("failed"), None),
		other => (other.to_owned(), None),
	}
}
