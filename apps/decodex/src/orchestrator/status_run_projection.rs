//! Operator run status projection, protocol/activity readback, and lane lifecycle metrics.

use super::{
	ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE, ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE,
	ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE, AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE,
	AUTHORITY_DECISION_REQUEST_EVENT_TYPE, AgentPrivateEvidenceRef,
	CONTINUATION_PENDING_RUN_STATUS, ChildAgentActivityBucket, ChildAgentActivitySummary,
	CodexAccountActivitySummary, Duration, EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH, HashMap,
	HashSet, OperatorArchitectureRecoveryStatus, OperatorAuthorityDecisionRequestStatus,
	OperatorBoundaryStatus, OperatorContinuationRecoveryStatus, OperatorHistoryLaneStatus,
	OperatorLaneControlProjection, OperatorLaneLifecycleAttemptEvidence,
	OperatorLaneLifecycleMetrics, OperatorLaneLifecyclePhaseMetrics, OperatorLifecycleMetricPhase,
	OperatorLoopStatus, OperatorPhaseAcceptanceStatus, OperatorRecoveryBudgetStatus,
	OperatorReviewCheckpointStatus, OperatorReviewCheckpointSummaryFields,
	OperatorReviewLoopStatus, OperatorReviewRouteCount, OperatorRunAppServerState,
	OperatorRunControlCapability, OperatorRunLifecycleProjection, OperatorRunProtocolSummary,
	OperatorRunStatus, OperatorRunTiming, OperatorTerminalFinalizeProjection,
	PHASE_ACCEPTANCE_CHECK_EVENT_TYPE, PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT,
	PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE, PHASE_GOAL_RECOVERY_EVENT_TYPE, PrivateExecutionEvent,
	ProjectLoopEvidenceSnapshot, ProjectRunStatus, ProtocolActivitySummary,
	REVIEW_POLICY_CONVERGENCE_BUDGET, RUN_LEASE_IDLE_TIMEOUT, RUN_OPERATION_AGENT_RUN,
	RUN_OPERATION_IDLE, RUN_OPERATION_REVIEW_WRITEBACK, RUN_OPERATION_WAITING_EXTERNAL,
	ReviewLevel, Rfc3339, RunActivityMarker, ServiceConfig, StateStore,
	TERMINAL_GUARDED_RUN_STATUS, Value, append_primary_account_if_missing,
	marker_process_liveness_for_marker, not_loaded_history_ledger_outcome, observed_idle_duration,
	operator_authority_decision_request_status_from_event, operator_autonomy_lineage_statuses,
	operator_autonomy_objective_status, operator_autonomy_proposal_statuses,
	operator_autonomy_report_status, operator_autonomy_signal_statuses,
	operator_run_counts_as_attention, operator_run_counts_as_running,
	operator_run_has_fresh_execution, operator_run_has_recent_app_server_execution,
	operator_run_needs_attention, private_evidence_ref_for_run_fields, public_text,
	relative_worktree_path_for_path, run_activity_idle_timeout, state,
};
use time::OffsetDateTime;

pub(super) fn operator_run_status(
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
fn operator_run_status_from_parts(
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

fn operator_run_active_goal_phase(events: &[PrivateExecutionEvent]) -> Option<String> {
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

fn operator_run_public_progress_phase(events: &[PrivateExecutionEvent]) -> Option<String> {
	events.iter().rev().find_map(|event| {
		(event.event_type() == "progress_checkpoint")
			.then_some(event.payload())
			.and_then(|payload| payload.get("phase"))
			.and_then(Value::as_str)
			.map(str::to_owned)
	})
}

fn operator_run_phase_acceptance_status(
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

fn hydrate_operator_run_derived_status(mut status: OperatorRunStatus) -> OperatorRunStatus {
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

fn operator_lane_control_state(run: &OperatorRunStatus) -> OperatorLaneControlProjection {
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

fn operator_run_ownership_state(
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

fn operator_run_is_continuation_wait(run: &OperatorRunStatus) -> bool {
	run.attempt_status == CONTINUATION_PENDING_RUN_STATUS
		|| run.phase == "waiting_continuation"
		|| run.retry_kind.as_deref() == Some("continuation")
		|| run.wait_reason.as_deref() == Some("continuation_retry")
}

fn operator_run_liveness_state(run: &OperatorRunStatus) -> String {
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

fn operator_run_policy_state(run: &OperatorRunStatus) -> String {
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

fn operator_run_terminalization_state(run: &OperatorRunStatus, liveness_state: &str) -> String {
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

fn operator_run_lane_control_conditions(
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

fn operator_run_lane_control_next_action(
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

fn operator_run_lifecycle_projection(
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

fn operator_run_wait_reason(
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

fn operator_run_accounts(
	marker: Option<&RunActivityMarker>,
) -> (Option<CodexAccountActivitySummary>, Vec<CodexAccountActivitySummary>) {
	let account = marker.and_then(RunActivityMarker::account).cloned();
	let mut accounts = marker.map(|marker| marker.accounts().to_vec()).unwrap_or_default();

	append_primary_account_if_missing(&mut accounts, account.as_ref());

	(account, accounts)
}

fn operator_run_relative_worktree_path(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
) -> Option<String> {
	run.worktree_path().map(|path| relative_worktree_path_for_path(project, path))
}

fn operator_run_private_evidence(
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

fn operator_run_loop_status(
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

fn operator_run_default_review_phase(
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

fn operator_run_lifecycle_loop_summary(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> Option<String> {
	operator_run_has_terminal_lifecycle(status, phase, current_operation)
		.then(|| format!("terminal lifecycle: {status}"))
}

fn operator_run_has_terminal_lifecycle(status: &str, phase: &str, current_operation: &str) -> bool {
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

pub(super) fn operator_loop_status_for_run(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_review_phase: Option<&str>,
	lifecycle_summary: Option<String>,
) -> crate::prelude::Result<OperatorLoopStatus> {
	let loop_evidence = state_store.project_loop_evidence_snapshot(project.service_id())?;

	operator_loop_status_for_run_with_evidence(
		project,
		&loop_evidence,
		issue_id,
		run_id,
		attempt_number,
		default_review_phase,
		lifecycle_summary,
	)
}

fn operator_loop_status_for_run_with_evidence(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_review_phase: Option<&str>,
	lifecycle_summary: Option<String>,
) -> crate::prelude::Result<OperatorLoopStatus> {
	let review_level = project.codex().review_level();
	let review = operator_review_loop_status(
		review_level,
		loop_evidence,
		issue_id,
		run_id,
		attempt_number,
		default_review_phase,
	)?;
	let events = loop_evidence.private_events(issue_id, run_id, attempt_number);
	let architecture_recovery =
		events.iter().rev().find_map(operator_architecture_recovery_status_from_event);
	let boundary = events.iter().rev().find_map(operator_boundary_status_from_event);
	let decision_request = events
		.iter()
		.rev()
		.find(|event| event.event_type() == AUTHORITY_DECISION_REQUEST_EVENT_TYPE)
		.and_then(operator_authority_decision_request_status_from_event);
	let autonomy_objective = operator_autonomy_objective_status(project, loop_evidence);
	let autonomy_signals = operator_autonomy_signal_statuses(loop_evidence);
	let autonomy_proposals = operator_autonomy_proposal_statuses(loop_evidence);
	let autonomy_lineage = operator_autonomy_lineage_statuses(loop_evidence);
	let autonomy_report = operator_autonomy_report_status(
		autonomy_objective.as_ref(),
		&autonomy_signals,
		&autonomy_proposals,
		&autonomy_lineage,
	);
	let autonomy = operator_loop_autonomy(
		boundary.as_ref(),
		architecture_recovery.as_ref(),
		decision_request.as_ref(),
	);
	let summary = operator_loop_status_summary(
		review.as_ref(),
		architecture_recovery.as_ref(),
		boundary.as_ref(),
		decision_request.as_ref(),
		autonomy,
		lifecycle_summary.as_deref(),
	);
	let next_action = operator_loop_status_next_action(
		review.as_ref(),
		architecture_recovery.as_ref(),
		boundary.as_ref(),
		decision_request.as_ref(),
	);

	Ok(OperatorLoopStatus {
		review_level: review_level.as_str().to_owned(),
		autonomy: autonomy.to_owned(),
		summary,
		next_action,
		autonomy_objective,
		autonomy_signals,
		autonomy_proposals,
		autonomy_lineage,
		autonomy_report,
		review,
		architecture_recovery,
		boundary,
		decision_request,
	})
}

fn operator_review_loop_status(
	review_level: ReviewLevel,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_review_phase: Option<&str>,
) -> crate::prelude::Result<Option<OperatorReviewLoopStatus>> {
	if let Some(checkpoint) = operator_latest_review_checkpoint_event_status(
		loop_evidence,
		issue_id,
		run_id,
		attempt_number,
	) {
		return Ok(Some(checkpoint));
	}

	let latest_checkpoint = ["handoff", "repair"]
		.into_iter()
		.filter_map(|phase| {
			loop_evidence.review_policy_checkpoint(issue_id, run_id, attempt_number, phase)
		})
		.max_by(|left, right| {
			left.updated_at_unix()
				.cmp(&right.updated_at_unix())
				.then_with(|| left.phase().cmp(right.phase()))
		});

	if let Some(checkpoint) = latest_checkpoint {
		let nonclean_rounds = checkpoint.nonclean_rounds();
		let summary = operator_review_checkpoint_summary_fields(checkpoint.details_json());

		return Ok(Some(OperatorReviewLoopStatus {
			phase: checkpoint.phase().to_owned(),
			status: checkpoint.status().to_owned(),
			checkpoint: Some(OperatorReviewCheckpointStatus {
				head_sha: checkpoint.head_sha().to_owned(),
				round: nonclean_rounds,
				nonclean_rounds,
				review_class: summary.review_class,
				risk_class: summary.risk_class,
				compact_eligible: summary.compact_eligible,
				fallback_reason: summary.fallback_reason,
				active_fingerprints: summary.active_fingerprints,
				stop_fingerprint: summary.stop_fingerprint,
				route_counts: summary.route_counts,
				route_next_action: summary.route_next_action,
				updated_at: checkpoint.updated_at().to_owned(),
			}),
		}));
	}

	if review_level.requires_review_checkpoint()
		&& let Some(default_review_phase) = default_review_phase
	{
		return Ok(Some(OperatorReviewLoopStatus {
			phase: default_review_phase.to_owned(),
			status: String::from("pending"),
			checkpoint: None,
		}));
	}

	Ok(None)
}

fn operator_latest_review_checkpoint_event_status(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
) -> Option<OperatorReviewLoopStatus> {
	loop_evidence.private_events(issue_id, run_id, attempt_number).iter().rev().find_map(|event| {
		let payload = event.payload();

		if event.event_type() != "review_checkpoint" {
			return None;
		}

		let phase = payload.get("phase").and_then(Value::as_str)?;
		let status = payload.get("status").and_then(Value::as_str)?;
		let head_sha = payload.get("head_sha").and_then(Value::as_str)?;
		let nonclean_rounds = payload.get("nonclean_rounds").and_then(Value::as_i64).unwrap_or(0);
		let checkpoint =
			loop_evidence.review_policy_checkpoint(issue_id, run_id, attempt_number, phase)?;

		if checkpoint.status() != status
			|| checkpoint.head_sha() != head_sha
			|| checkpoint.nonclean_rounds() != nonclean_rounds
		{
			return None;
		}

		let details_json = payload.get("review").unwrap_or(payload).to_string();
		let summary = operator_review_checkpoint_summary_fields(&details_json);

		Some(OperatorReviewLoopStatus {
			phase: phase.to_owned(),
			status: status.to_owned(),
			checkpoint: Some(OperatorReviewCheckpointStatus {
				head_sha: head_sha.to_owned(),
				round: nonclean_rounds,
				nonclean_rounds,
				review_class: summary.review_class,
				risk_class: summary.risk_class,
				compact_eligible: summary.compact_eligible,
				fallback_reason: summary.fallback_reason,
				active_fingerprints: summary.active_fingerprints,
				stop_fingerprint: summary.stop_fingerprint,
				route_counts: summary.route_counts,
				route_next_action: summary.route_next_action,
				updated_at: checkpoint.updated_at().to_owned(),
			}),
		})
	})
}

fn operator_review_checkpoint_summary_fields(
	details_json: &str,
) -> OperatorReviewCheckpointSummaryFields {
	let Ok(details) = serde_json::from_str::<Value>(details_json) else {
		return OperatorReviewCheckpointSummaryFields {
			review_class: None,
			risk_class: None,
			compact_eligible: None,
			fallback_reason: None,
			active_fingerprints: Vec::new(),
			stop_fingerprint: None,
			route_counts: Vec::new(),
			route_next_action: None,
		};
	};
	let policy = details.get("finding_policy");
	let cost_control = details.get("review_cost_control");
	let review_class = cost_control
		.and_then(|cost_control| cost_control.get("review_class"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let risk_class = cost_control
		.and_then(|cost_control| cost_control.get("risk_class"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let compact_eligible = cost_control
		.and_then(|cost_control| cost_control.get("compact_eligible"))
		.and_then(Value::as_bool);
	let fallback_reason = cost_control
		.and_then(|cost_control| cost_control.get("fallback_reason"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let active_fingerprints = policy
		.and_then(|policy| policy.get("active_fingerprints"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(str::to_owned)
		.collect();
	let stop_fingerprint = policy
		.and_then(|policy| policy.get("stop_fingerprint"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let route_summary = details.get("finding_route_summary");
	let route_counts = route_summary
		.and_then(|summary| summary.get("route_counts"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|count| {
			Some(OperatorReviewRouteCount {
				route: count.get("route")?.as_str()?.to_owned(),
				count: usize::try_from(count.get("count")?.as_u64()?).ok()?,
			})
		})
		.collect();
	let route_next_action = route_summary
		.and_then(|summary| summary.get("next_action"))
		.and_then(Value::as_str)
		.map(str::to_owned);

	OperatorReviewCheckpointSummaryFields {
		review_class,
		risk_class,
		compact_eligible,
		fallback_reason,
		active_fingerprints,
		stop_fingerprint,
		route_counts,
		route_next_action,
	}
}

fn operator_architecture_recovery_status_from_event(
	event: &PrivateExecutionEvent,
) -> Option<OperatorArchitectureRecoveryStatus> {
	if !matches!(
		event.event_type(),
		ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE
			| ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE
			| ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE
	) {
		return None;
	}

	let payload = event.payload();
	let reason_code = payload.get("reason_code")?.as_str()?.to_owned();
	let guardrail_reason = payload
		.get("guardrail_reason")
		.and_then(Value::as_str)
		.or_else(|| {
			payload
				.get("loop_guardrail")
				.and_then(|guardrail| guardrail.get("reason"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned);
	let boundary_disposition = payload
		.get("boundary_disposition")
		.and_then(Value::as_str)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("disposition"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned);
	let boundary_policy_decision = payload
		.get("boundary_policy_decision")
		.and_then(Value::as_str)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("policy_decision"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned)
		.or_else(|| {
			boundary_disposition
				.as_deref()
				.map(operator_boundary_policy_decision_from_disposition)
				.map(str::to_owned)
		});
	let requires_enhanced_evidence = payload
		.get("requires_enhanced_evidence")
		.and_then(Value::as_bool)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("requires_enhanced_evidence"))
				.and_then(Value::as_bool)
		})
		.unwrap_or_else(|| {
			boundary_policy_decision
				.as_deref()
				.is_some_and(operator_boundary_policy_requires_enhanced_evidence)
		});
	let blocks_landing = payload
		.get("blocks_landing")
		.and_then(Value::as_bool)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("blocks_landing"))
				.and_then(Value::as_bool)
		})
		.unwrap_or_else(|| {
			boundary_policy_decision.as_deref().is_some_and(operator_boundary_policy_blocks_landing)
		});
	let recovery_budget_attempt = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("attempt"))
		.and_then(Value::as_u64);
	let recovery_budget_max_attempts = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("max_attempts"))
		.and_then(Value::as_u64);
	let budget = recovery_budget_attempt
		.zip(recovery_budget_max_attempts)
		.map(|(attempt, max_attempts)| OperatorRecoveryBudgetStatus { attempt, max_attempts });
	let next_action = operator_architecture_recovery_next_action(
		&reason_code,
		boundary_policy_decision.as_deref(),
		requires_enhanced_evidence,
		blocks_landing,
	);

	Some(OperatorArchitectureRecoveryStatus {
		status: operator_architecture_recovery_status_for_reason(&reason_code).to_owned(),
		reason_code,
		guardrail_reason,
		boundary_disposition,
		boundary_policy_decision,
		requires_enhanced_evidence,
		blocks_landing,
		round: recovery_budget_attempt,
		budget,
		next_action,
	})
}

fn operator_architecture_recovery_status_for_reason(reason_code: &str) -> &'static str {
	match reason_code {
		"architecture_recovery_started" => "active",
		"architecture_recovery_exhausted" => "exhausted",
		"contract_boundary_required" | "external_dependency_required" => "human_required",
		_ => "terminal",
	}
}

fn operator_architecture_recovery_next_action(
	reason_code: &str,
	policy_decision: Option<&str>,
	requires_enhanced_evidence: bool,
	blocks_landing: bool,
) -> String {
	match reason_code {
		"architecture_recovery_started" => {
			match (policy_decision, blocks_landing, requires_enhanced_evidence) {
				(Some(policy), true, _) => format!(
					"Retry with a materially different implementation strategy under authority policy `{policy}`; keep landing blocked until validation or review-policy evidence is restored."
				),
				(Some(policy), false, true) => format!(
					"Retry with a materially different implementation strategy under authority policy `{policy}`; preserve enhanced evidence before review handoff or landing."
				),
				(Some(policy), false, false) => format!(
					"Retry with a materially different implementation strategy under authority policy `{policy}`."
				),
				(None, true, _) => String::from(
					"Retry with a materially different implementation strategy; keep landing blocked until validation or review-policy evidence is restored.",
				),
				(None, false, true) => String::from(
					"Retry with a materially different implementation strategy; preserve enhanced evidence before review handoff or landing.",
				),
				(None, false, false) => String::from(
					"Retry with a materially different implementation strategy inside authority.",
				),
			}
		},
		"architecture_recovery_exhausted" => String::from(
			"Require a new accepted recovery strategy or architecture decision before retrying.",
		),
		"external_dependency_required" => String::from(
			"Resolve the dependency or Execution Program readiness blocker before retrying.",
		),
		"contract_boundary_required" => String::from(
			"Resolve the Decision Contract or Authority Envelope boundary before retrying.",
		),
		_ => String::from("Inspect the Architecture Recovery Packet before retrying."),
	}
}

fn operator_boundary_policy_decision_from_disposition(disposition: &str) -> &'static str {
	match disposition {
		"requires_human" | "insufficient_evidence" => "requires_human_decision",
		_ => "auto_continue",
	}
}

pub(super) fn operator_boundary_policy_requires_enhanced_evidence(policy_decision: &str) -> bool {
	matches!(policy_decision, "requires_enhanced_evidence" | "block_landing")
}

pub(super) fn operator_boundary_policy_blocks_landing(policy_decision: &str) -> bool {
	policy_decision == "block_landing"
}

fn operator_boundary_status_from_event(
	event: &PrivateExecutionEvent,
) -> Option<OperatorBoundaryStatus> {
	if event.event_type() != AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE {
		return None;
	}

	let payload = event.payload();
	let disposition = payload
		.get("final_disposition")
		.and_then(|final_disposition| final_disposition.get("disposition"))
		.and_then(Value::as_str)
		.or_else(|| payload.get("disposition").and_then(Value::as_str))?
		.to_owned();
	let reason = payload
		.get("final_disposition")
		.and_then(|final_disposition| final_disposition.get("reason"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let policy_decision = payload
		.get("policy_decision")
		.and_then(Value::as_str)
		.or_else(|| {
			payload.get("policy").and_then(|policy| policy.get("decision")).and_then(Value::as_str)
		})
		.map(str::to_owned)
		.unwrap_or_else(|| {
			operator_boundary_policy_decision_from_disposition(&disposition).to_owned()
		});
	let attempted_recovery_reason =
		payload.get("attempted_recovery_reason").and_then(Value::as_str).map(str::to_owned);
	let changed_surface_count =
		payload.get("changed_surfaces").and_then(Value::as_array).map_or(0, Vec::len);
	let improvement_signal_count =
		payload.get("improvement_signals").and_then(Value::as_array).map_or(0, Vec::len);
	let requires_enhanced_evidence = payload
		.get("policy")
		.and_then(|policy| policy.get("requires_enhanced_evidence"))
		.and_then(Value::as_bool)
		.unwrap_or_else(|| operator_boundary_policy_requires_enhanced_evidence(&policy_decision));
	let blocks_landing = payload
		.get("policy")
		.and_then(|policy| policy.get("blocks_landing"))
		.and_then(Value::as_bool)
		.unwrap_or_else(|| operator_boundary_policy_blocks_landing(&policy_decision));

	Some(OperatorBoundaryStatus {
		disposition,
		policy_decision,
		reason,
		attempted_recovery_reason,
		changed_surface_count,
		improvement_signal_count,
		requires_enhanced_evidence,
		blocks_landing,
	})
}

fn operator_loop_autonomy(
	boundary: Option<&OperatorBoundaryStatus>,
	architecture_recovery: Option<&OperatorArchitectureRecoveryStatus>,
	decision_request: Option<&OperatorAuthorityDecisionRequestStatus>,
) -> &'static str {
	if decision_request.is_some() {
		return "human_required";
	}
	if boundary.is_some_and(|boundary| boundary.policy_decision == "requires_human_decision") {
		return "human_required";
	}
	if architecture_recovery.is_some_and(|recovery| recovery.status != "active") {
		return "human_required";
	}

	"autonomous"
}

fn operator_loop_status_summary(
	review: Option<&OperatorReviewLoopStatus>,
	architecture_recovery: Option<&OperatorArchitectureRecoveryStatus>,
	boundary: Option<&OperatorBoundaryStatus>,
	decision_request: Option<&OperatorAuthorityDecisionRequestStatus>,
	autonomy: &str,
	lifecycle_summary: Option<&str>,
) -> String {
	if let Some(request) = decision_request {
		return format!("human-required boundary stop: {} on {}", request.reason, request.boundary);
	}
	if let Some(recovery) = architecture_recovery {
		return format!("architecture recovery {}: {}", recovery.status, recovery.reason_code);
	}
	if let Some(review) = review {
		if let Some(fingerprint) =
			review.checkpoint.as_ref().and_then(|checkpoint| checkpoint.stop_fingerprint.as_ref())
		{
			return format!(
				"review {}: {} stopped on fingerprint {}",
				review.phase, review.status, fingerprint
			);
		}

		return format!("review {}: {}", review.phase, review.status);
	}
	if let Some(boundary) = boundary {
		return format!("boundary check: {}", boundary.disposition);
	}
	if let Some(lifecycle_summary) = lifecycle_summary {
		return lifecycle_summary.to_owned();
	}

	format!("loop autonomy: {autonomy}")
}

fn operator_loop_status_next_action(
	review: Option<&OperatorReviewLoopStatus>,
	architecture_recovery: Option<&OperatorArchitectureRecoveryStatus>,
	boundary: Option<&OperatorBoundaryStatus>,
	decision_request: Option<&OperatorAuthorityDecisionRequestStatus>,
) -> Option<String> {
	if let Some(request) = decision_request {
		return Some(request.next_action.clone());
	}
	if let Some(recovery) = architecture_recovery {
		return Some(recovery.next_action.clone());
	}
	if let Some(boundary) = boundary {
		return match boundary.policy_decision.as_str() {
			"requires_human_decision" =>
				Some(String::from("Resolve the Authority Boundary Check before retrying the lane.")),
			"block_landing" => Some(String::from(
				"Continue recovery, but block landing until review or validation policy evidence is restored.",
			)),
			"requires_enhanced_evidence" => Some(String::from(
				"Continue recovery and preserve enhanced evidence before review handoff or landing.",
			)),
			_ => None,
		};
	}

	review.and_then(|review| {
		if review.status != "clean"
			&& let Some(route_next_action) = review
				.checkpoint
				.as_ref()
				.and_then(|checkpoint| checkpoint.route_next_action.clone())
		{
			return Some(route_next_action);
		}

		match review.status.as_str() {
			"clean" if review.phase == "handoff" => Some(String::from(
				"Push or update the PR and record review handoff for the clean current lane head.",
			)),
			"clean" if review.phase == "repair" => Some(String::from(
				"Record a fresh current-head handoff review checkpoint for the repaired lane head.",
			)),
			"pending" => Some(String::from(
				"Record the independent Decodex Review checkpoint for the current lane head.",
			)),
			"findings" => Some(String::from(
				"Repair validated review findings and record a fresh checkpoint.",
			)),
			"blocked" =>
				Some(String::from("Resolve the blocked Decodex Review before continuing.")),
			"needs_architecture_review" =>
				Some(String::from("Get architecture direction before continuing review repair.")),
			_ => None,
		}
	})
}

fn operator_run_control_capability(
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

fn load_operator_run_marker(
	run: &ProjectRunStatus,
) -> crate::prelude::Result<Option<RunActivityMarker>> {
	let marker = run.worktree_path().and_then(|worktree_path| {
		state::read_run_activity_marker_snapshot(worktree_path).unwrap_or_default()
	});

	Ok(marker.filter(|marker| {
		marker.run_id() == run.run_id() && marker.attempt_number() == run.attempt_number()
	}))
}

fn operator_run_timing(
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

fn operator_run_app_server_state(
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

fn operator_run_protocol_summary(
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

fn marker_protocol_summary_supersedes_run(
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

fn operator_run_terminal_finalize_projection(
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

fn review_handoff_terminal_finalize_wait_reason(
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

fn review_repair_terminal_finalize_wait_reason(
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

fn operator_run_continuation_recovery_status(
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

fn operator_continuation_recovery_event_status(
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

fn operator_run_visible_status(
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

fn operator_run_status_projection_reason(
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

fn operator_run_live_evidence_source(
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

fn operator_run_has_recent_protocol_execution_evidence(
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
) -> bool {
	operator_protocol_event_counts_as_live_execution(protocol_summary.last_event_type.as_deref())
		&& timing.protocol_idle_for_seconds.is_some_and(|idle_for| {
			u64::try_from(idle_for)
				.is_ok_and(|idle_for| idle_for < RUN_LEASE_IDLE_TIMEOUT.as_secs())
		})
}

fn operator_protocol_event_counts_as_live_execution(event_type: Option<&str>) -> bool {
	let Some(event_type) = event_type else {
		return false;
	};

	state::protocol_event_counts_as_work_progress(event_type)
		&& !matches!(event_type.to_ascii_lowercase().as_str(), "thread/archive" | "turn/completed")
}

fn operator_run_has_app_server_execution_evidence(
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

fn operator_run_queue_lease_state(run_lease: bool) -> String {
	if run_lease { String::from("held") } else { String::from("not_held") }
}

fn operator_run_execution_liveness(
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

fn process_liveness_reason_is_identity_mismatch(reason: Option<&str>) -> bool {
	matches!(reason, Some("host_boot_id_mismatch" | "process_start_identity_mismatch"))
}

fn operator_run_child_agent_activity(
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

fn operator_run_protocol_activity(
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

fn sanitize_operator_protocol_activity_summary(summary: &mut ProtocolActivitySummary) {
	for event in &mut summary.recent_events {
		if let Some(detail) = event.detail.as_deref()
			&& !operator_protocol_activity_detail_is_public(detail)
		{
			event.detail = Some(String::from("redacted_sensitive_detail"));
		}
	}
}

pub(super) fn operator_protocol_activity_detail_is_public(detail: &str) -> bool {
	public_text::validate_public_text_field("protocol_activity.detail", detail).is_ok()
		&& !contains_protocol_activity_host_path_shape(detail)
		&& !contains_protocol_activity_secret_shape(detail)
}

fn contains_protocol_activity_host_path_shape(detail: &str) -> bool {
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

fn contains_protocol_activity_secret_shape(detail: &str) -> bool {
	detail.split(protocol_activity_token_separator).any(|token| {
		let normalized = token.to_ascii_lowercase();

		normalized.starts_with("ghp_")
			|| normalized.starts_with("github_pat_")
			|| is_high_entropy_protocol_activity_token(token)
	})
}

fn protocol_activity_token_separator(character: char) -> bool {
	!(character.is_ascii_alphanumeric() || character == '_' || character == '-')
}

fn is_high_entropy_protocol_activity_token(token: &str) -> bool {
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

fn protocol_wait_reason_from_child_bucket(current_bucket: &str) -> String {
	match current_bucket {
		"Model" => String::from("model_execution"),
		"Protocol" => String::from("protocol_activity"),
		_ => String::from("tool_execution"),
	}
}

fn idle_duration_seconds(
	last_activity_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
) -> Option<i64> {
	last_activity_unix_epoch
		.and_then(|last_activity| now_unix_epoch.checked_sub(last_activity))
		.filter(|idle_for| *idle_for >= 0)
}

fn max_optional_i64(left: Option<i64>, right: Option<i64>) -> Option<i64> {
	match (left, right) {
		(Some(left), Some(right)) => Some(left.max(right)),
		(Some(value), None) | (None, Some(value)) => Some(value),
		(None, None) => None,
	}
}

pub(super) fn format_optional_unix_timestamp(unix_epoch: Option<i64>) -> Option<String> {
	unix_epoch.and_then(|unix_epoch| {
		OffsetDateTime::from_unix_timestamp(unix_epoch)
			.ok()
			.and_then(|timestamp| timestamp.format(&Rfc3339).ok())
	})
}

pub(super) fn format_optional_i64(value: Option<i64>) -> String {
	value.map_or_else(|| String::from("none"), |value| value.to_string())
}

fn classify_operator_run_operation(phase: &str, marker_current_operation: Option<&str>) -> String {
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

fn operator_run_is_suspected_stall(
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

fn suspected_operator_run_stall_threshold(idle_timeout: Duration) -> Duration {
	Duration::from_secs((idle_timeout.as_secs() / 2).max(1))
}

fn operator_run_progress_diagnostic(
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

fn operator_latest_repo_gate_failure_progress_diagnostic(
	private_events: &[PrivateExecutionEvent],
) -> Option<String> {
	private_events
		.iter()
		.rev()
		.find(|event| event.event_type() == "phase_goal_transition")
		.and_then(operator_repo_gate_failure_progress_diagnostic)
}

fn operator_repo_gate_failure_progress_diagnostic(event: &PrivateExecutionEvent) -> Option<String> {
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

fn protocol_activity_is_non_work_only(protocol_activity: &ProtocolActivitySummary) -> bool {
	!protocol_activity.recent_events.is_empty()
		&& protocol_activity
			.recent_events
			.iter()
			.all(|event| !state::protocol_event_counts_as_work_progress(&event.event_type))
}

fn visible_operator_run_retry_schedule(
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

fn classify_operator_run_phase(
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

pub(super) fn operator_history_lanes(
	current_lanes: &[OperatorRunStatus],
	recent_runs: &[OperatorRunStatus],
) -> Vec<OperatorHistoryLaneStatus> {
	let current_lane_run_ids =
		current_lanes.iter().map(|run| run.run_id.as_str()).collect::<HashSet<_>>();
	let current_lane_issue_ids =
		current_lanes.iter().map(|run| run.issue_id.as_str()).collect::<HashSet<_>>();
	let mut lane_indexes = HashMap::new();
	let mut lanes = Vec::new();

	for run in recent_runs {
		if current_lane_run_ids.contains(run.run_id.as_str())
			|| current_lane_issue_ids.contains(run.issue_id.as_str())
		{
			continue;
		}

		let group_key = operator_run_group_key(run);

		if let Some(index) = lane_indexes.get(&group_key) {
			let lane: &mut OperatorHistoryLaneStatus = &mut lanes[*index];

			lane.attempt_count += 1;

			if run.attempt_number > lane.latest_run.attempt_number {
				lane.latest_run = run.clone();
			}

			hydrate_history_lane_from_run(lane, run);

			lane.attempts.push(run.clone());

			lane.lifecycle_metrics = operator_lane_lifecycle_metrics(&lane.attempts);

			continue;
		}

		lane_indexes.insert(group_key, lanes.len());

		let attempts = vec![run.clone()];
		let lifecycle_metrics = operator_lane_lifecycle_metrics(&attempts);

		lanes.push(OperatorHistoryLaneStatus {
			project_id: run.project_id.clone(),
			issue_id: run.issue_id.clone(),
			issue_identifier: run.issue_identifier.clone(),
			title: run.title.clone(),
			author: run.author.clone(),
			issue_state: None,
			active_label_present: None,
			needs_attention_label_present: None,
			issue_key: operator_run_issue_key(run),
			attempt_count: 1,
			ledger_outcome: not_loaded_history_ledger_outcome(),
			lifecycle_metrics,
			latest_run: run.clone(),
			attempts,
		});
	}

	lanes
}

pub(super) fn hydrate_history_lane_from_run(
	lane: &mut OperatorHistoryLaneStatus,
	run: &OperatorRunStatus,
) {
	if lane.issue_identifier.is_none()
		&& let Some(issue_identifier) =
			run.issue_identifier.as_ref().filter(|value| !value.trim().is_empty())
	{
		lane.issue_identifier = Some(issue_identifier.clone());
		lane.issue_key = issue_identifier.clone();
	}
	if lane.title.is_none() {
		lane.title = run.title.clone();
	}
	if lane.author.is_none() {
		lane.author = run.author.clone();
	}
}

pub(super) fn hydrate_current_lane_lifecycle_metrics(
	project: &ServiceConfig,
	state_store: &StateStore,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	project_display_name: &str,
	current_lanes: &mut [OperatorRunStatus],
	recent_runs: &[OperatorRunStatus],
	now_unix_epoch: i64,
) -> crate::prelude::Result<()> {
	for current_lane in current_lanes {
		let attempts = current_lane_lifecycle_attempts(
			project,
			state_store,
			loop_evidence,
			project_display_name,
			current_lane,
			recent_runs,
			now_unix_epoch,
		)?;

		current_lane.lifecycle_metrics = operator_lane_lifecycle_metrics(&attempts);
	}

	Ok(())
}

fn current_lane_lifecycle_attempts(
	project: &ServiceConfig,
	state_store: &StateStore,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	project_display_name: &str,
	current_lane: &OperatorRunStatus,
	recent_runs: &[OperatorRunStatus],
	now_unix_epoch: i64,
) -> crate::prelude::Result<Vec<OperatorRunStatus>> {
	let issue_runs =
		state_store.list_project_issue_runs(project.service_id(), &current_lane.issue_id)?;
	let mut attempts = issue_runs
		.into_iter()
		.map(|run| {
			operator_run_status(project, loop_evidence, project_display_name, run, now_unix_epoch)
		})
		.collect::<crate::prelude::Result<Vec<_>>>()?;

	if attempts.is_empty() {
		let group_key = operator_run_group_key(current_lane);

		attempts.extend(
			recent_runs.iter().filter(|run| operator_run_group_key(run) == group_key).cloned(),
		);
	}

	let current_lane_snapshot = operator_run_current_lane_snapshot_attempt(current_lane);

	if let Some(attempt) = attempts.iter_mut().find(|run| run.run_id == current_lane.run_id) {
		*attempt = current_lane_snapshot;
	} else {
		attempts.push(current_lane_snapshot);
	}

	Ok(attempts)
}

fn operator_run_current_lane_snapshot_attempt(run: &OperatorRunStatus) -> OperatorRunStatus {
	let mut snapshot = run.clone();
	let mut evidence = std::collections::BTreeSet::<String>::new();

	evidence.insert(String::from("current_lane_snapshot"));
	evidence.extend(snapshot.lifecycle_evidence.iter().cloned());

	snapshot.lifecycle_source = String::from("current_snapshot");
	snapshot.lifecycle_evidence = evidence.into_iter().collect();

	snapshot
}

pub(super) fn operator_lane_lifecycle_metrics(
	attempts: &[OperatorRunStatus],
) -> OperatorLaneLifecycleMetrics {
	let mut metrics = operator_lane_lifecycle_totals(attempts.iter());

	metrics.phases = operator_lane_lifecycle_phase_metrics(attempts);

	metrics
}

fn operator_lane_lifecycle_totals<'a>(
	runs: impl IntoIterator<Item = &'a OperatorRunStatus>,
) -> OperatorLaneLifecycleMetrics {
	let mut bucket_totals = HashMap::<String, ChildAgentActivityBucket>::new();
	let mut warning_set = HashSet::<String>::new();
	let mut run_ids = HashSet::<String>::new();
	let mut metrics = OperatorLaneLifecycleMetrics::default();

	for run in runs {
		metrics.attempt_count += 1;

		run_ids.insert(run.run_id.clone());

		match run.lifecycle_source.as_str() {
			"recorded" => metrics.recorded_attempt_count += 1,
			"recovered" => metrics.recovered_attempt_count += 1,
			"current_snapshot" => metrics.current_snapshot_attempt_count += 1,
			_ => {},
		}

		metrics.recovery_gaps.extend(run.lifecycle_gaps.iter().cloned());
		metrics.attempt_evidence.push(operator_lane_lifecycle_attempt_evidence(run));

		metrics.protocol_event_count =
			metrics.protocol_event_count.saturating_add(run.event_count.max(0));

		let Some(summary) = run.child_agent_activity.as_ref() else {
			continue;
		};

		metrics.captured_attempt_count += 1;
		metrics.child_event_count =
			metrics.child_event_count.saturating_add(summary.event_count.max(0));
		metrics.wall_seconds = metrics.wall_seconds.saturating_add(summary.wall_seconds.max(0));
		metrics.tool_call_count =
			metrics.tool_call_count.saturating_add(summary.tool_call_count.max(0));
		metrics.input_tokens_current =
			max_optional_i64(metrics.input_tokens_current, summary.input_tokens_current);
		metrics.input_tokens_peak =
			max_optional_i64(metrics.input_tokens_peak, summary.input_tokens_max);
		metrics.input_tokens_cumulative =
			metrics.input_tokens_cumulative.saturating_add(summary.input_tokens_cumulative.max(0));
		metrics.output_tokens_cumulative = metrics
			.output_tokens_cumulative
			.saturating_add(summary.output_tokens_cumulative.max(0));

		if summary.largest_tool_output_bytes.is_some_and(|bytes| {
			metrics.largest_tool_output_bytes.is_none_or(|current| bytes > current)
		}) {
			metrics.largest_tool_output_bytes = summary.largest_tool_output_bytes;
			metrics.largest_tool_output_tool = summary.largest_tool_output_tool.clone();
		}

		for warning in &summary.large_output_warnings {
			if !warning.trim().is_empty() {
				warning_set.insert(warning.clone());
			}
		}
		for bucket in &summary.buckets {
			let total = bucket_totals.entry(bucket.name.clone()).or_insert_with(|| {
				ChildAgentActivityBucket {
					name: bucket.name.clone(),
					..ChildAgentActivityBucket::default()
				}
			});

			total.wall_seconds = total.wall_seconds.saturating_add(bucket.wall_seconds.max(0));
			total.event_count = total.event_count.saturating_add(bucket.event_count.max(0));
			total.tool_call_count =
				total.tool_call_count.saturating_add(bucket.tool_call_count.max(0));
			total.input_tokens = total.input_tokens.saturating_add(bucket.input_tokens.max(0));
			total.output_tokens = total.output_tokens.saturating_add(bucket.output_tokens.max(0));
			total.output_bytes = total.output_bytes.saturating_add(bucket.output_bytes.max(0));
		}
	}

	metrics.missing_attempt_count =
		metrics.attempt_count.saturating_sub(metrics.captured_attempt_count);
	metrics.run_count = run_ids.len();
	metrics.large_output_warnings = warning_set.into_iter().collect();

	metrics.recovery_gaps.sort();
	metrics.recovery_gaps.dedup();
	metrics.attempt_evidence.sort_by(|left, right| {
		left.attempt_number.cmp(&right.attempt_number).then_with(|| left.run_id.cmp(&right.run_id))
	});
	metrics.large_output_warnings.sort();

	metrics.buckets = bucket_totals.into_values().collect();

	metrics.buckets.sort_by(|left, right| {
		right
			.wall_seconds
			.cmp(&left.wall_seconds)
			.then_with(|| right.event_count.cmp(&left.event_count))
			.then_with(|| left.name.cmp(&right.name))
	});

	metrics
}

fn operator_lane_lifecycle_phase_metrics(
	attempts: &[OperatorRunStatus],
) -> Vec<OperatorLaneLifecyclePhaseMetrics> {
	let mut groups = HashMap::<String, (String, u8, Vec<&OperatorRunStatus>)>::new();

	for run in attempts {
		let phase = operator_run_lifecycle_metric_phase(run);
		let entry = groups
			.entry(phase.key.to_owned())
			.or_insert_with(|| (phase.label.to_owned(), phase.rank, Vec::new()));

		entry.2.push(run);
	}

	let mut phases = groups
		.into_iter()
		.map(|(phase, (label, rank, runs))| {
			let totals = operator_lane_lifecycle_totals(runs);

			(
				rank,
				OperatorLaneLifecyclePhaseMetrics {
					phase,
					label,
					attempt_count: totals.attempt_count,
					run_count: totals.run_count,
					recorded_attempt_count: totals.recorded_attempt_count,
					recovered_attempt_count: totals.recovered_attempt_count,
					current_snapshot_attempt_count: totals.current_snapshot_attempt_count,
					captured_attempt_count: totals.captured_attempt_count,
					missing_attempt_count: totals.missing_attempt_count,
					protocol_event_count: totals.protocol_event_count,
					child_event_count: totals.child_event_count,
					wall_seconds: totals.wall_seconds,
					tool_call_count: totals.tool_call_count,
					input_tokens_current: totals.input_tokens_current,
					input_tokens_peak: totals.input_tokens_peak,
					input_tokens_cumulative: totals.input_tokens_cumulative,
					output_tokens_cumulative: totals.output_tokens_cumulative,
					largest_tool_output_bytes: totals.largest_tool_output_bytes,
					largest_tool_output_tool: totals.largest_tool_output_tool,
					large_output_warnings: totals.large_output_warnings,
					buckets: totals.buckets,
					attempt_evidence: totals.attempt_evidence,
					recovery_gaps: totals.recovery_gaps,
				},
			)
		})
		.collect::<Vec<_>>();

	phases.sort_by(|(left_rank, left), (right_rank, right)| {
		left_rank.cmp(right_rank).then_with(|| left.phase.cmp(&right.phase))
	});

	phases.into_iter().map(|(_rank, phase)| phase).collect()
}

fn operator_run_lifecycle_metric_phase(run: &OperatorRunStatus) -> OperatorLifecycleMetricPhase {
	if matches!(
		run.status.as_str(),
		"cleanup_complete" | "closeout" | "closeout_pending" | "landed"
	) {
		return operator_lifecycle_metric_phase("closeout", "Closeout", 30);
	}
	if matches!(
		run.status.as_str(),
		"manual_attention" | "manual_attention_pending" | "needs_attention" | "terminal_failure"
	) || run.phase == "needs_attention"
	{
		return operator_lifecycle_metric_phase("manual_attention", "Manual attention", 40);
	}

	if let Some(review) = run
		.loop_status
		.as_ref()
		.and_then(|status| status.review.as_ref())
		.filter(|review| review.checkpoint.is_some() || review.status != "pending")
	{
		return match review.phase.as_str() {
			"repair" => operator_lifecycle_metric_phase("review_repair", "Review repair", 20),
			_ => operator_lifecycle_metric_phase("review", "Review", 10),
		};
	}

	if run.status == "review_repair_pending" {
		return operator_lifecycle_metric_phase("review_repair", "Review repair", 20);
	}
	if run.status == "review_handoff_pending"
		|| run.current_operation == RUN_OPERATION_REVIEW_WRITEBACK
	{
		return operator_lifecycle_metric_phase("review", "Review", 10);
	}

	operator_lifecycle_metric_phase("development", "Development", 0)
}

fn operator_lane_lifecycle_attempt_evidence(
	run: &OperatorRunStatus,
) -> OperatorLaneLifecycleAttemptEvidence {
	let phase = operator_run_lifecycle_metric_phase(run);
	let child_event_count =
		run.child_agent_activity.as_ref().map(|summary| summary.event_count.max(0)).unwrap_or(0);

	OperatorLaneLifecycleAttemptEvidence {
		run_id: run.run_id.clone(),
		issue_id: run.issue_id.clone(),
		attempt_number: run.attempt_number,
		status: run.status.clone(),
		phase: phase.key.to_owned(),
		source: run.lifecycle_source.clone(),
		evidence: run.lifecycle_evidence.clone(),
		gaps: run.lifecycle_gaps.clone(),
		protocol_event_count: run.event_count.max(0),
		child_event_count,
		updated_at: run.updated_at.clone(),
	}
}

fn operator_lifecycle_metric_phase(
	key: &'static str,
	label: &'static str,
	rank: u8,
) -> OperatorLifecycleMetricPhase {
	OperatorLifecycleMetricPhase { key, label, rank }
}

pub(super) fn operator_run_group_key(run: &OperatorRunStatus) -> String {
	let issue_id = run.issue_id.trim();

	if !issue_id.is_empty() && !issue_id.eq_ignore_ascii_case("unknown") {
		return issue_id.to_ascii_uppercase();
	}

	operator_run_issue_key(run)
}

pub(super) fn operator_run_issue_key(run: &OperatorRunStatus) -> String {
	if let Some(issue_identifier) = run
		.issue_identifier
		.as_ref()
		.filter(|value| !value.trim().is_empty() && !value.eq_ignore_ascii_case("unknown"))
	{
		return issue_identifier.clone();
	}
	if let Some(issue_identifier) = operator_run_issue_identifier_from_fields(
		&run.run_id,
		run.branch_name.as_deref(),
		run.worktree_path.as_deref(),
	) {
		return issue_identifier;
	}

	let issue_id = run.issue_id.trim();

	if issue_id.is_empty() { String::from("unknown") } else { issue_id.to_owned() }
}

pub(super) fn operator_run_issue_identifier_from_fields(
	run_id: &str,
	branch_name: Option<&str>,
	worktree_path: Option<&str>,
) -> Option<String> {
	if let Some(issue_identifier) = issue_identifier_from_run_id(run_id) {
		return Some(issue_identifier);
	}

	for value in [branch_name, worktree_path] {
		if let Some(issue_identifier) = value.and_then(issue_identifier_in_text) {
			return Some(issue_identifier);
		}
	}

	None
}

fn issue_identifier_from_run_id(run_id: &str) -> Option<String> {
	if let Some((candidate, _attempt_suffix)) = run_id.split_once("-attempt-") {
		return issue_identifier_in_text(candidate);
	}
	if let Some(candidate) = run_id.strip_prefix("recovered-") {
		return issue_identifier_in_text(candidate);
	}

	None
}

pub(super) fn issue_identifier_in_text(value: &str) -> Option<String> {
	let bytes = value.as_bytes();

	for index in 0..bytes.len() {
		if !bytes[index].is_ascii_alphabetic() {
			continue;
		}

		let mut prefix_end = index + 1;

		while prefix_end < bytes.len() && bytes[prefix_end].is_ascii_alphanumeric() {
			prefix_end += 1;
		}

		if prefix_end >= bytes.len() || bytes[prefix_end] != b'-' {
			continue;
		}

		let mut digit_end = prefix_end + 1;

		while digit_end < bytes.len() && bytes[digit_end].is_ascii_digit() {
			digit_end += 1;
		}

		if digit_end > prefix_end + 1 {
			return Some(value[index..digit_end].to_ascii_uppercase());
		}
	}

	None
}
