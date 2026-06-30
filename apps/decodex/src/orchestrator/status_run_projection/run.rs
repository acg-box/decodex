#[allow(clippy::wildcard_imports)]
use super::*;

pub(in crate::orchestrator) fn operator_run_status(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	project_display_name: &str,
	run: ProjectRunStatus,
	now_unix_epoch: i64,
) -> crate::prelude::Result<OperatorRunStatus> {
	let marker = load_operator_run_marker(&run)?;
	let timing = operator_run_timing(&run, marker.as_ref(), now_unix_epoch);
	let app_server_state = operator_run_app_server_state(&run, marker.as_ref());
	let protocol_summary = operator_run_protocol_summary(&run, marker.as_ref());
	let terminal_finalize_projection =
		operator_run_terminal_finalize_projection(loop_evidence, &run);
	let lifecycle = operator_run_lifecycle_projection(
		&run,
		marker.as_ref(),
		terminal_finalize_projection,
		&timing,
		&app_server_state,
		&protocol_summary,
		now_unix_epoch,
	);
	let child_agent_activity = operator_run_child_agent_activity(
		marker.as_ref(),
		run.child_agent_activity(),
		now_unix_epoch,
	);
	let protocol_activity = operator_run_protocol_activity(
		marker.as_ref(),
		run.protocol_activity(),
		&app_server_state,
		child_agent_activity.as_ref(),
		timing.protocol_idle_for_seconds,
		matches!(lifecycle.status.as_str(), "starting" | "running"),
	);
	let wait_reason = operator_run_wait_reason(
		&lifecycle.phase,
		lifecycle.wait_reason.clone(),
		protocol_activity.as_ref(),
	);
	let private_events =
		loop_evidence.private_events(run.issue_id(), run.run_id(), run.attempt_number());
	let progress_diagnostic = operator_run_progress_diagnostic(
		&lifecycle.phase,
		&timing,
		protocol_activity.as_ref(),
		private_events,
		now_unix_epoch,
		run_activity_idle_timeout(marker.as_ref()),
	);
	let (account, accounts) = operator_run_accounts(marker.as_ref());
	let branch_name = run.branch_name().map(str::to_owned);
	let worktree_path = operator_run_relative_worktree_path(project, &run);
	let issue_identifier = operator_run_issue_identifier_from_fields(
		run.run_id(),
		branch_name.as_deref(),
		worktree_path.as_deref(),
	);
	let private_evidence =
		operator_run_private_evidence(project, &run, issue_identifier.as_deref());
	let continuation_recovery = operator_run_continuation_recovery_status(loop_evidence, &run);
	let active_goal_phase = operator_run_active_goal_phase(private_events);
	let public_progress_phase = operator_run_public_progress_phase(private_events);
	let phase_acceptance = operator_run_phase_acceptance_status(private_events);
	let loop_status = operator_run_loop_status(
		project,
		loop_evidence,
		&run,
		&lifecycle.status,
		&lifecycle.phase,
		&lifecycle.current_operation,
	)?;

	Ok(hydrate_operator_run_derived_status(operator_run_status_from_parts(
		project,
		project_display_name,
		&run,
		lifecycle,
		wait_reason,
		app_server_state,
		timing,
		protocol_summary,
		child_agent_activity,
		protocol_activity,
		progress_diagnostic,
		account,
		accounts,
		branch_name,
		worktree_path,
		issue_identifier,
		private_evidence,
		continuation_recovery,
		phase_acceptance,
		active_goal_phase,
		public_progress_phase,
		loop_status,
	)))
}

#[allow(clippy::too_many_arguments)]
pub(in crate::orchestrator) fn operator_run_status_from_parts(
	project: &ServiceConfig,
	project_display_name: &str,
	run: &ProjectRunStatus,
	lifecycle: OperatorRunLifecycleProjection,
	wait_reason: Option<String>,
	app_server_state: OperatorRunAppServerState,
	timing: OperatorRunTiming,
	protocol_summary: OperatorRunProtocolSummary,
	child_agent_activity: Option<ChildAgentActivitySummary>,
	protocol_activity: Option<ProtocolActivitySummary>,
	progress_diagnostic: Option<String>,
	account: Option<CodexAccountActivitySummary>,
	accounts: Vec<CodexAccountActivitySummary>,
	branch_name: Option<String>,
	worktree_path: Option<String>,
	issue_identifier: Option<String>,
	private_evidence: AgentPrivateEvidenceRef,
	continuation_recovery: Option<OperatorContinuationRecoveryStatus>,
	phase_acceptance: Option<OperatorPhaseAcceptanceStatus>,
	active_goal_phase: Option<String>,
	public_progress_phase: Option<String>,
	loop_status: OperatorLoopStatus,
) -> OperatorRunStatus {
	let run_phase = lifecycle.phase.clone();

	OperatorRunStatus {
		project_id: project.service_id().to_owned(),
		project_display_name: project_display_name.to_owned(),
		run_id: run.run_id().to_owned(),
		issue_id: run.issue_id().to_owned(),
		issue_identifier,
		title: None,
		author: None,
		issue_state: None,
		active_label_present: None,
		needs_attention_label_present: None,
		attempt_number: run.attempt_number(),
		status: lifecycle.status,
		attempt_status: run.status().to_owned(),
		status_projection_reason: lifecycle.status_projection_reason,
		ownership_state: String::new(),
		liveness_state: String::new(),
		policy_state: String::new(),
		terminalization_state: String::new(),
		lane_control_next_action: String::new(),
		lane_control_conditions: Vec::new(),
		phase: lifecycle.phase,
		run_phase,
		wait_reason,
		current_operation: lifecycle.current_operation,
		active_goal_phase,
		public_progress_phase,
		control_capability: operator_run_control_capability(run, &app_server_state),
		thread_id: app_server_state.thread_id,
		turn_id: app_server_state.turn_id,
		thread_status: app_server_state.thread_status,
		thread_active_flags: app_server_state.thread_active_flags,
		interactive_requested: app_server_state.interactive_requested,
		continuation_pending: app_server_state.continuation_pending,
		continuation_recovery,
		phase_acceptance,
		run_lease: lifecycle.run_lease,
		queue_lease_state: operator_run_queue_lease_state(lifecycle.run_lease),
		execution_liveness: lifecycle.execution_liveness,
		has_fresh_execution: false,
		counts_as_running: false,
		needs_attention: false,
		updated_at: run.updated_at().to_owned(),
		last_run_activity_at: format_optional_unix_timestamp(timing.last_run_activity_unix_epoch),
		last_protocol_activity_at: format_optional_unix_timestamp(
			timing.last_protocol_activity_unix_epoch,
		),
		last_progress_at: format_optional_unix_timestamp(timing.last_progress_unix_epoch),
		idle_for_seconds: timing.idle_for_seconds,
		protocol_idle_for_seconds: timing.protocol_idle_for_seconds,
		suspected_stall: lifecycle.suspected_stall,
		progress_diagnostic,
		last_event_type: protocol_summary.last_event_type,
		last_event_at: protocol_summary.last_event_at,
		event_count: protocol_summary.event_count,
		private_evidence,
		loop_status: Some(loop_status),
		process_id: timing.process_id,
		process_alive: timing.process_alive,
		process_liveness_reason: timing.process_liveness_reason,
		retry_kind: lifecycle.retry_kind,
		next_retry_at: format_optional_unix_timestamp(lifecycle.retry_ready_at_unix_epoch),
		effective_model: app_server_state.effective_model,
		effective_model_provider: app_server_state.effective_model_provider,
		effective_cwd: app_server_state.effective_cwd,
		effective_approval_policy: app_server_state.effective_approval_policy,
		effective_approvals_reviewer: app_server_state.effective_approvals_reviewer,
		effective_sandbox_mode: app_server_state.effective_sandbox_mode,
		child_agent_activity,
		protocol_activity,
		lifecycle_source: run.recovery_source().to_owned(),
		lifecycle_evidence: run.recovery_evidence().to_vec(),
		lifecycle_gaps: run.recovery_gaps().to_vec(),
		lifecycle_metrics: OperatorLaneLifecycleMetrics::default(),
		account,
		accounts,
		branch_name,
		worktree_path,
	}
}

pub(in crate::orchestrator) fn operator_run_active_goal_phase(
	events: &[PrivateExecutionEvent],
) -> Option<String> {
	for event in events.iter().rev() {
		if matches!(event.event_type(), "phase_goal_completed" | "phase_goal_transition") {
			return None;
		}
		if !matches!(event.event_type(), "phase_goal_set" | "phase_goal_status") {
			continue;
		}

		let payload = event.payload();
		let nested = payload.get("payload").unwrap_or(payload);
		let status = nested.get("status").or_else(|| payload.get("status")).and_then(Value::as_str);

		if status.is_some_and(|value| matches!(value, "complete" | "completed" | "blocked")) {
			return None;
		}

		return nested
			.get("phase")
			.or_else(|| payload.get("phase"))
			.and_then(Value::as_str)
			.map(str::to_owned);
	}

	None
}

pub(in crate::orchestrator) fn operator_run_public_progress_phase(
	events: &[PrivateExecutionEvent],
) -> Option<String> {
	events.iter().rev().find_map(|event| {
		(event.event_type() == "progress_checkpoint")
			.then_some(event.payload())
			.and_then(|payload| payload.get("phase"))
			.and_then(Value::as_str)
			.map(str::to_owned)
	})
}

pub(in crate::orchestrator) fn operator_run_phase_acceptance_status(
	events: &[PrivateExecutionEvent],
) -> Option<OperatorPhaseAcceptanceStatus> {
	let event = events
		.iter()
		.rev()
		.find(|event| event.event_type() == PHASE_ACCEPTANCE_CHECK_EVENT_TYPE)?;
	let payload = event.payload();
	let phase = payload.get("phase")?.as_str()?.to_owned();
	let decision = payload.get("decision")?.as_str()?.to_owned();
	let reason_code = payload.get("reason_code")?.as_str()?.to_owned();
	let objective_covered = payload
		.get("objective_coverage")
		.and_then(|objective| objective.get("covered"))
		.and_then(Value::as_bool)
		.unwrap_or(false);
	let effective_delta_present = payload
		.get("effective_delta")
		.and_then(|delta| delta.get("present"))
		.and_then(Value::as_bool)
		.unwrap_or(false);
	let changed_surfaces = payload
		.get("effective_delta")
		.and_then(|delta| delta.get("changed_surfaces"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(str::to_owned)
		.collect();
	let non_goal_passed = payload
		.get("non_goal_check")
		.and_then(|check| check.get("passed"))
		.and_then(Value::as_bool)
		.unwrap_or(false);
	let validation_passed = payload
		.get("validation_evidence")
		.and_then(|evidence| evidence.get("repo_gate_passed"))
		.and_then(Value::as_bool)
		.unwrap_or(false);

	Some(OperatorPhaseAcceptanceStatus {
		phase,
		decision,
		reason_code,
		objective_covered,
		effective_delta_present,
		changed_surfaces,
		non_goal_passed,
		validation_passed,
		recorded_at: event.recorded_at().to_owned(),
		run_id: event.run_id().to_owned(),
		attempt_number: event.attempt_number(),
		next_action: payload
			.get("next_action")
			.and_then(Value::as_str)
			.unwrap_or("inspect_phase_acceptance_check")
			.to_owned(),
	})
}

pub(in crate::orchestrator) fn hydrate_operator_run_derived_status(
	mut status: OperatorRunStatus,
) -> OperatorRunStatus {
	status.has_fresh_execution = operator_run_has_fresh_execution(&status);
	status.needs_attention = operator_run_needs_attention(&status);

	let lane_control_state = operator_lane_control_state(&status);

	status.ownership_state = lane_control_state.ownership_state;
	status.liveness_state = lane_control_state.liveness_state;
	status.policy_state = lane_control_state.policy_state;
	status.terminalization_state = lane_control_state.terminalization_state;
	status.lane_control_next_action = lane_control_state.next_action;
	status.lane_control_conditions = lane_control_state.conditions;
	status.needs_attention = operator_run_counts_as_attention(&status);
	status.counts_as_running = operator_run_counts_as_running(&status);

	status
}

pub(in crate::orchestrator) fn operator_lane_control_state(
	run: &OperatorRunStatus,
) -> OperatorLaneControlProjection {
	let liveness_state = operator_run_liveness_state(run);
	let policy_state = operator_run_policy_state(run);
	let terminalization_state = operator_run_terminalization_state(run, &liveness_state);
	let ownership_state =
		operator_run_ownership_state(run, &liveness_state, &policy_state, &terminalization_state);
	let next_action = operator_run_lane_control_next_action(
		run,
		&ownership_state,
		&liveness_state,
		&policy_state,
		&terminalization_state,
	);
	let mut conditions = operator_run_lane_control_conditions(run, &liveness_state, &policy_state);

	if ownership_state == "leased_run" && !run.run_lease {
		conditions.push(String::from("invalid_leased_run_without_lease"));
	}

	OperatorLaneControlProjection {
		ownership_state,
		liveness_state,
		policy_state,
		terminalization_state,
		next_action,
		conditions,
	}
}

pub(in crate::orchestrator) fn operator_run_ownership_state(
	run: &OperatorRunStatus,
	liveness_state: &str,
	policy_state: &str,
	terminalization_state: &str,
) -> String {
	if run.run_lease
		&& matches!(run.attempt_status.as_str(), "starting" | "running" | "continuation_pending")
		&& !matches!(
			policy_state,
			"review_churn_exceeded"
				| "continuation_recovery_churn_exceeded"
				| "authority_boundary_required"
				| "human_attention_required"
		) {
		return String::from("leased_run");
	}
	if matches!(
		policy_state,
		"review_churn_exceeded"
			| "continuation_recovery_churn_exceeded"
			| "authority_boundary_required"
			| "human_attention_required"
	) || run.needs_attention
		|| (!run.run_lease && liveness_state == "host_boot_mismatch")
	{
		return String::from("retained_attention");
	}
	if operator_run_is_continuation_wait(run) {
		return String::from("continuation_pending");
	}
	if !run.run_lease
		&& matches!(liveness_state, "process_alive" | "thread_active" | "protocol_recent")
	{
		return String::from("orphaned_live_thread");
	}
	if terminalization_state != "none" && terminalization_state != "cleanup_complete" {
		return String::from("terminalizing");
	}
	if matches!(run.attempt_status.as_str(), "starting" | "running" | "continuation_pending") {
		return String::from("pending");
	}

	String::from("closed")
}

pub(in crate::orchestrator) fn operator_run_is_continuation_wait(run: &OperatorRunStatus) -> bool {
	run.attempt_status == CONTINUATION_PENDING_RUN_STATUS
		|| run.phase == "waiting_continuation"
		|| run.retry_kind.as_deref() == Some("continuation")
		|| run.wait_reason.as_deref() == Some("continuation_retry")
}

pub(in crate::orchestrator) fn operator_run_liveness_state(run: &OperatorRunStatus) -> String {
	if matches!(run.process_liveness_reason.as_deref(), Some("host_boot_id_mismatch")) {
		return String::from("host_boot_mismatch");
	}
	if run.process_alive == Some(true) {
		return String::from("process_alive");
	}
	if run.process_alive == Some(false)
		|| matches!(run.execution_liveness.as_str(), "not_running" | "process_identity_mismatch")
	{
		return String::from("not_running");
	}
	if matches!(run.thread_status.as_deref(), Some("active")) || !run.thread_active_flags.is_empty()
	{
		return String::from("thread_active");
	}
	if operator_run_has_recent_app_server_execution(run) {
		return String::from("protocol_recent");
	}

	String::from("unknown")
}

pub(in crate::orchestrator) fn operator_run_policy_state(run: &OperatorRunStatus) -> String {
	if run.continuation_recovery.as_ref().is_some_and(|recovery| recovery.budget_exceeded) {
		return String::from("continuation_recovery_churn_exceeded");
	}

	let Some(loop_status) = run.loop_status.as_ref() else {
		return String::from("allowed");
	};

	if loop_status.decision_request.is_some() {
		return String::from("authority_boundary_required");
	}
	if loop_status.autonomy == "human_required" {
		return String::from("human_attention_required");
	}

	if let Some(recovery) = loop_status.architecture_recovery.as_ref() {
		return if recovery.status == "active" {
			String::from("architecture_recovery_pending")
		} else {
			String::from("human_attention_required")
		};
	}
	if let Some(review) = loop_status.review.as_ref() {
		return match review.status.as_str() {
			"pending" => String::from("review_pending"),
			"findings" => {
				if review.checkpoint.as_ref().is_some_and(|checkpoint| {
					checkpoint.nonclean_rounds >= REVIEW_POLICY_CONVERGENCE_BUDGET
				}) {
					String::from("review_churn_exceeded")
				} else {
					String::from("review_findings")
				}
			},
			"blocked" | "needs_architecture_review" => String::from("human_attention_required"),
			_ => String::from("allowed"),
		};
	}

	String::from("allowed")
}

pub(in crate::orchestrator) fn operator_run_terminalization_state(
	run: &OperatorRunStatus,
	liveness_state: &str,
) -> String {
	if matches!(run.status.as_str(), "cleanup_complete" | "merged_closeout_reconciled")
		|| matches!(run.current_operation.as_str(), "ledger_outcome")
			&& matches!(run.phase.as_str(), "completed")
	{
		return String::from("cleanup_complete");
	}
	if matches!(run.phase.as_str(), "completed" | "failed" | "terminated")
		&& !run.run_lease
		&& matches!(liveness_state, "not_running" | "unknown")
	{
		return String::from("cleanup_complete");
	}
	if matches!(run.phase.as_str(), "completed" | "failed" | "terminated") {
		return String::from("barrier_started");
	}

	String::from("none")
}

pub(in crate::orchestrator) fn operator_run_lane_control_conditions(
	run: &OperatorRunStatus,
	liveness_state: &str,
	policy_state: &str,
) -> Vec<String> {
	let mut conditions = Vec::new();

	if !run.run_lease
		&& matches!(run.attempt_status.as_str(), "starting" | "running" | "continuation_pending")
	{
		conditions.push(String::from("run_lease_missing"));
	}
	if matches!(run.attempt_status.as_str(), "failed" | "interrupted" | "stalled" | "succeeded")
		&& matches!(liveness_state, "process_alive" | "thread_active" | "protocol_recent")
	{
		conditions.push(String::from("terminal_attempt_has_live_evidence"));
	}
	if liveness_state == "host_boot_mismatch" {
		conditions.push(String::from("host_boot_id_mismatch"));
	}
	if policy_state == "review_churn_exceeded" {
		conditions.push(String::from("review_churn_threshold_exceeded"));
	}
	if policy_state == "continuation_recovery_churn_exceeded" {
		conditions.push(String::from("continuation_recovery_budget_exceeded"));
	}
	if matches!(policy_state, "authority_boundary_required" | "human_attention_required") {
		conditions.push(String::from("policy_requires_human_attention"));
	}

	conditions
}

pub(in crate::orchestrator) fn operator_run_lane_control_next_action(
	run: &OperatorRunStatus,
	ownership_state: &str,
	liveness_state: &str,
	policy_state: &str,
	terminalization_state: &str,
) -> String {
	if policy_state == "review_churn_exceeded" {
		return String::from("start_architecture_recovery_or_stop_for_human_attention");
	}
	if policy_state == "continuation_recovery_churn_exceeded" {
		return String::from("stop_auto_continuation_and_request_architecture_recovery");
	}
	if matches!(policy_state, "authority_boundary_required" | "human_attention_required") {
		return String::from("resolve_policy_stop_before_mutating_lane");
	}
	if ownership_state == "orphaned_live_thread" {
		return String::from("inspect_or_interrupt_orphaned_live_thread");
	}
	if liveness_state == "host_boot_mismatch" {
		return String::from("inspect_recovery_evidence");
	}
	if terminalization_state != "none" && terminalization_state != "cleanup_complete" {
		return String::from("finish_terminalization");
	}
	if ownership_state == "leased_run" {
		if let Some(next_action) =
			run.loop_status.as_ref().and_then(|loop_status| loop_status.next_action.clone())
		{
			return next_action;
		}

		return String::from("continue_owned_attempt");
	}
	if ownership_state == "continuation_pending" {
		return String::from("wait_for_continuation_reentry");
	}
	if ownership_state == "closed" {
		return String::from("no_action");
	}

	if let Some(next_action) =
		run.loop_status.as_ref().and_then(|loop_status| loop_status.next_action.clone())
	{
		return next_action;
	}

	String::from("inspect_lane_state")
}

pub(in crate::orchestrator) fn operator_run_lifecycle_projection(
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
			operator_run_visible_status(
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
		operator_run_status_projection_reason(
			run.status(),
			&status,
			app_server_state,
			protocol_summary,
			timing,
			marker_current_operation,
		)
	};
	let (retry_kind, retry_ready_at_unix_epoch) = visible_operator_run_retry_schedule(
		&status,
		marker.and_then(RunActivityMarker::retry_kind),
		marker.and_then(RunActivityMarker::retry_ready_at_unix_epoch),
		now_unix_epoch,
	);
	let (phase, wait_reason) = if let Some(projection) = terminal_finalize_projection {
		(String::from(projection.phase), Some(String::from(projection.wait_reason)))
	} else {
		classify_operator_run_phase(
			&status,
			retry_kind.as_deref(),
			retry_ready_at_unix_epoch,
			now_unix_epoch,
		)
	};
	let current_operation = terminal_finalize_projection
		.map(|projection| projection.current_operation.to_owned())
		.unwrap_or_else(|| classify_operator_run_operation(&phase, marker_current_operation));
	let suspected_stall = terminal_finalize_projection.is_none()
		&& operator_run_is_suspected_stall(
			&phase,
			timing.last_progress_unix_epoch,
			now_unix_epoch,
			run_activity_idle_timeout(marker),
		);
	let execution_liveness = if terminal_finalize_projection.is_some() {
		String::from("not_running")
	} else {
		operator_run_execution_liveness(&status, timing, app_server_state, protocol_summary)
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

pub(in crate::orchestrator) fn operator_run_wait_reason(
	phase: &str,
	wait_reason: Option<String>,
	protocol_activity: Option<&ProtocolActivitySummary>,
) -> Option<String> {
	if wait_reason.is_some() || phase != "executing" {
		return wait_reason;
	}

	protocol_activity
		.and_then(|summary| summary.waiting_reason.clone())
		.filter(|reason| reason != "turn_completed")
}

pub(in crate::orchestrator) fn operator_run_accounts(
	marker: Option<&RunActivityMarker>,
) -> (Option<CodexAccountActivitySummary>, Vec<CodexAccountActivitySummary>) {
	let account = marker.and_then(RunActivityMarker::account).cloned();
	let mut accounts = marker.map(|marker| marker.accounts().to_vec()).unwrap_or_default();

	append_primary_account_if_missing(&mut accounts, account.as_ref());

	(account, accounts)
}

pub(in crate::orchestrator) fn operator_run_relative_worktree_path(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
) -> Option<String> {
	run.worktree_path().map(|path| relative_worktree_path_for_path(project, path))
}

pub(in crate::orchestrator) fn operator_run_private_evidence(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
) -> AgentPrivateEvidenceRef {
	private_evidence_ref_for_run_fields(
		project.service_id(),
		project.config_path(),
		run.issue_id(),
		issue_identifier,
		run.run_id(),
		run.attempt_number(),
	)
}

pub(in crate::orchestrator) fn operator_run_loop_status(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
	status: &str,
	phase: &str,
	current_operation: &str,
) -> crate::prelude::Result<OperatorLoopStatus> {
	operator_loop_status_for_run_with_evidence(
		project,
		loop_evidence,
		run.issue_id(),
		run.run_id(),
		run.attempt_number(),
		operator_run_default_review_phase(status, phase, current_operation),
		operator_run_lifecycle_loop_summary(status, phase, current_operation),
	)
}

pub(in crate::orchestrator) fn operator_run_default_review_phase(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> Option<&'static str> {
	if operator_run_has_terminal_lifecycle(status, phase, current_operation) {
		return None;
	}
	if current_operation == RUN_OPERATION_REVIEW_WRITEBACK {
		return Some("handoff");
	}

	None
}

pub(in crate::orchestrator) fn operator_run_lifecycle_loop_summary(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> Option<String> {
	operator_run_has_terminal_lifecycle(status, phase, current_operation)
		.then(|| format!("terminal lifecycle: {status}"))
}

pub(in crate::orchestrator) fn operator_run_has_terminal_lifecycle(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> bool {
	phase == "completed"
		|| phase == "terminal_pending"
		|| current_operation == "ledger_outcome"
		|| matches!(
			status,
			"succeeded"
				| "failed" | "interrupted"
				| "review_handoff_pending"
				| "review_repair_pending"
				| "closeout_pending"
				| "manual_attention_pending"
				| "cleanup_complete"
				| "closeout" | "landed"
				| "manual_attention"
				| TERMINAL_GUARDED_RUN_STATUS
		)
}
