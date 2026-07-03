use crate::orchestrator::{
	self, EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH, OperatorContinuationRecoveryStatus,
	OperatorHistoryLaneStatus, OperatorHistoryLedgerOutcome, OperatorLaneLifecycleMetrics,
	OperatorPhaseAcceptanceStatus, OperatorRunStatus, status_render::activity,
};

pub(super) fn append_rendered_history_lane(output: &mut String, lane: &OperatorHistoryLaneStatus) {
	output.push_str(&format!(
		"- issue: {}\n  project_id: {}\n  issue_id: {}\n  issue_identifier: {}\n  title: {}\n  attempts: {}\n  ledger_status: {}\n  outcome: {}\n",
		lane.issue_key,
		lane.project_id,
		lane.issue_id,
		lane.issue_identifier.as_deref().unwrap_or("none"),
		lane.title.as_deref().unwrap_or("none"),
		lane.attempt_count,
		lane.ledger_outcome.ledger_status,
		lane.ledger_outcome.final_outcome
	));

	append_rendered_history_ledger_outcome(output, &lane.ledger_outcome);

	output.push_str(&format!(
		"  lifecycle_metrics: {}\n",
		render_lane_lifecycle_metrics(&lane.lifecycle_metrics)
	));

	if history_ledger_outcome_has_records(&lane.ledger_outcome) {
		output.push_str(&format!(
			"  local_attempts: {}\n  latest_run_id: {}\n",
			lane.attempt_count, lane.latest_run.run_id
		));
	} else {
		append_rendered_run(output, &lane.latest_run);
	}
	if lane.lifecycle_metrics.phases.is_empty() {
		return;
	}

	output.push_str("  lifecycle_bucket_breakdown:\n");

	for phase in &lane.lifecycle_metrics.phases {
		output.push_str(&format!(
			"    - lifecycle_bucket: {} lifecycle_bucket_key: {} attempts: {} sources: recorded={} recovered={} current_snapshot={} captured: {}/{} protocol_events: {} child_events: {} wall: {} tool_calls: {} input_tokens: {} output_tokens: {}\n",
			phase.label,
			phase.phase,
			phase.attempt_count,
			phase.recorded_attempt_count,
			phase.recovered_attempt_count,
			phase.current_snapshot_attempt_count,
			phase.captured_attempt_count,
			phase.attempt_count,
			phase.protocol_event_count,
			phase.child_event_count,
			activity::format_seconds_compact(phase.wall_seconds),
			phase.tool_call_count,
			phase.input_tokens_cumulative,
			phase.output_tokens_cumulative,
		));
	}
}

pub(super) fn append_rendered_run(output: &mut String, run: &OperatorRunStatus) {
	append_rendered_run_impl(output, run);
}

fn render_lane_lifecycle_metrics(metrics: &OperatorLaneLifecycleMetrics) -> String {
	format!(
		"attempts={}; sources=recorded:{},recovered:{},current_snapshot:{}; captured={}/{}; missing={}; protocol_events={}; child_events={}; wall={}; tool_calls={}; input_tokens={}; output_tokens={}",
		metrics.attempt_count,
		metrics.recorded_attempt_count,
		metrics.recovered_attempt_count,
		metrics.current_snapshot_attempt_count,
		metrics.captured_attempt_count,
		metrics.attempt_count,
		metrics.missing_attempt_count,
		metrics.protocol_event_count,
		metrics.child_event_count,
		activity::format_seconds_compact(metrics.wall_seconds),
		metrics.tool_call_count,
		metrics.input_tokens_cumulative,
		metrics.output_tokens_cumulative,
	)
}

fn render_lane_lifecycle_evidence(metrics: &OperatorLaneLifecycleMetrics) -> String {
	if metrics.attempt_evidence.is_empty() && metrics.recovery_gaps.is_empty() {
		return String::from("none");
	}

	let mut lines = metrics
		.attempt_evidence
		.iter()
		.map(|attempt| {
			let evidence = if attempt.evidence.is_empty() {
				String::from("none")
			} else {
				attempt.evidence.join(",")
			};
			let gaps = if attempt.gaps.is_empty() {
				String::from("none")
			} else {
				attempt.gaps.join(",")
			};

			format!(
				"run={} attempt={} phase={} source={} evidence={} gaps={} protocol_events={} child_events={} updated_at={}",
				attempt.run_id,
				attempt.attempt_number,
				attempt.phase,
				attempt.source,
				evidence,
				gaps,
				attempt.protocol_event_count,
				attempt.child_event_count,
				attempt.updated_at
			)
		})
		.collect::<Vec<_>>();

	if !metrics.recovery_gaps.is_empty() {
		lines.push(format!("aggregate_gaps={}", metrics.recovery_gaps.join(",")));
	}

	lines.join(" | ")
}

fn append_rendered_history_ledger_outcome(
	output: &mut String,
	outcome: &OperatorHistoryLedgerOutcome,
) {
	append_rendered_history_field(output, "event_type", outcome.final_event_type.as_deref());
	append_rendered_history_field(output, "event_at", outcome.final_event_at.as_deref());
	append_rendered_history_field(output, "summary", outcome.summary.as_deref());
	append_rendered_history_field(output, "pr_url", outcome.pr_url.as_deref());
	append_rendered_history_field(output, "commit_sha", outcome.commit_sha.as_deref());
	append_rendered_history_field(output, "branch", outcome.branch.as_deref());
	append_rendered_history_field(output, "closeout_status", outcome.closeout_status.as_deref());
	append_rendered_history_field(
		output,
		"needs_attention_reason",
		outcome.needs_attention_reason.as_deref(),
	);
	append_rendered_history_field(
		output,
		"lifecycle_started_at",
		outcome.lifecycle_started_at.as_deref(),
	);
	append_rendered_history_field(
		output,
		"lifecycle_finished_at",
		outcome.lifecycle_finished_at.as_deref(),
	);

	if let Some(elapsed) = outcome.lifecycle_elapsed_seconds {
		output.push_str(&format!("  lifecycle_elapsed_seconds: {elapsed}\n"));
	}

	output.push_str(&format!("  ledger_records: {}\n", outcome.record_count));
}

fn append_rendered_history_field(output: &mut String, label: &str, value: Option<&str>) {
	if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
		output.push_str(&format!("  {label}: {value}\n"));
	}
}

fn history_ledger_outcome_has_records(outcome: &OperatorHistoryLedgerOutcome) -> bool {
	matches!(outcome.ledger_status.as_str(), "present" | "partial")
}

fn operator_run_phase_readback(run: &OperatorRunStatus) -> &str {
	if run.run_phase.trim().is_empty() { &run.phase } else { &run.run_phase }
}

fn append_rendered_run_impl(output: &mut String, run: &OperatorRunStatus) {
	let (freshness_source, freshness_at) = operator_run_freshness(run);
	let protocol_event = render_run_protocol_event(run);
	let thread_id = run.thread_id.as_deref().unwrap_or("none");
	let turn_id = run.turn_id.as_deref().unwrap_or("none");
	let thread_status = run.thread_status.as_deref().unwrap_or("none");
	let thread_active_flags = render_run_thread_active_flags(run);
	let idle_for_seconds =
		run.idle_for_seconds.map_or_else(|| String::from("none"), |value| value.to_string());
	let protocol_idle_for_seconds = run
		.protocol_idle_for_seconds
		.map_or_else(|| String::from("none"), |value| value.to_string());
	let branch_name = run.branch_name.as_deref().unwrap_or("none");
	let worktree_path = run.worktree_path.as_deref().unwrap_or("none");
	let queue_lease = operator_run_queue_lease_summary(run);
	let child_agent_activity =
		activity::render_child_agent_activity_summary(run.child_agent_activity.as_ref());
	let context_pressure =
		activity::render_child_agent_context_pressure(run.child_agent_activity.as_ref());
	let protocol_activity =
		activity::render_protocol_activity_summary(run.protocol_activity.as_ref());
	let account = activity::render_account_summary(run.account.as_ref());
	let accounts = activity::render_accounts_summary(&run.accounts);
	let private_evidence = orchestrator::render_private_evidence_reference(run);
	let loop_status = activity::render_loop_status_summary(run.loop_status.as_ref());
	let loop_autonomy_signals =
		activity::render_loop_autonomy_signals_summary(run.loop_status.as_ref());
	let loop_review = activity::render_loop_review_summary(run.loop_status.as_ref());
	let loop_architecture_recovery =
		activity::render_loop_architecture_recovery_summary(run.loop_status.as_ref());
	let loop_boundary = activity::render_loop_boundary_summary(run.loop_status.as_ref());
	let control_capability =
		activity::render_control_capability_summary(run.control_capability.as_ref());
	let continuation_recovery =
		render_continuation_recovery_summary(run.continuation_recovery.as_ref());
	let phase_acceptance = render_phase_acceptance_summary(run.phase_acceptance.as_ref());

	output.push_str(&format!(
		"- run_id: {}\n  project_id: {}\n  issue_id: {}\n  issue_identifier: {}\n  title: {}\n  attempt: {}\n  status: {}\n  attempt_status: {}\n  status_projection_reason: {}\n  ownership_state: {}\n  liveness_state: {}\n  policy_state: {}\n  terminalization_state: {}\n  lane_control_next_action: {}\n  lane_control_conditions: {}\n  run_phase: {}\n  wait_reason: {}\n  current_operation: {}\n  active_goal_phase: {}\n  public_progress_phase: {}\n  run_lease: {}\n  queue_lease_state: {}\n  queue_lease: {}\n  execution_liveness: {}\n  has_fresh_execution: {}\n  counts_as_running: {}\n  needs_attention: {}\n  freshness_at: {}\n  freshness_source: {}\n  timing: run_idle={} protocol_idle={} last_progress={} protocol_event={} events={}\n  account: {}\n  accounts: {}\n  child_agent_activity: {}\n  protocol_activity: {}\n  context_pressure: {}\n  lifecycle_metrics: {}\n  lifecycle_evidence: {}\n  private_evidence: {}\n  loop_status: {}\n  loop_autonomy_signals: {}\n  loop_review: {}\n  loop_architecture_recovery: {}\n  loop_boundary: {}\n  control_capability: {}\n  thread_id: {}\n  turn_id: {}\n  thread_status: {}\n  thread_active_flags: {}\n  interactive_requested: {}\n  continuation_pending: {}\n  continuation_recovery: {}\n  phase_acceptance: {}\n  branch: {}\n  worktree_path: {}\n  updated_at: {}\n  last_run_activity_at: {}\n  last_protocol_activity_at: {}\n  last_progress_at: {}\n  idle_for_seconds: {}\n  protocol_idle_for_seconds: {}\n  suspected_stall: {}\n  progress_diagnostic: {}\n  process_id: {}\n  process_alive: {}\n  process_liveness_reason: {}\n  retry_kind: {}\n  next_retry_at: {}\n  effective_model: {}\n  effective_model_provider: {}\n  effective_cwd: {}\n  effective_approval_policy: {}\n  effective_approvals_reviewer: {}\n  effective_sandbox_mode: {}\n  protocol_event: {}\n  event_count: {}\n",
		run.run_id,
		run.project_id,
		run.issue_id,
		run.issue_identifier.as_deref().unwrap_or("none"),
		run.title.as_deref().unwrap_or("none"),
		run.attempt_number,
		run.status,
		run.attempt_status,
		run.status_projection_reason.as_deref().unwrap_or("none"),
		run.ownership_state,
		run.liveness_state,
		run.policy_state,
		run.terminalization_state,
		run.lane_control_next_action,
		render_lane_control_conditions(run),
		operator_run_phase_readback(run),
		run.wait_reason.as_deref().unwrap_or("none"),
		run.current_operation,
		run.active_goal_phase.as_deref().unwrap_or("none"),
		run.public_progress_phase.as_deref().unwrap_or("none"),
		if run.run_lease { "yes" } else { "no" },
		run.queue_lease_state,
		queue_lease,
		run.execution_liveness,
		if run.has_fresh_execution { "yes" } else { "no" },
		if run.counts_as_running { "yes" } else { "no" },
		if run.needs_attention { "yes" } else { "no" },
		freshness_at,
		freshness_source,
		idle_for_seconds,
		protocol_idle_for_seconds,
		run.last_progress_at.as_deref().unwrap_or("none"),
		protocol_event,
		run.event_count,
		account,
		accounts,
		child_agent_activity,
		protocol_activity,
		context_pressure,
		render_lane_lifecycle_metrics(&run.lifecycle_metrics),
		render_lane_lifecycle_evidence(&run.lifecycle_metrics),
		private_evidence,
		loop_status,
		loop_autonomy_signals,
		loop_review,
		loop_architecture_recovery,
		loop_boundary,
		control_capability,
		thread_id,
		turn_id,
		thread_status,
		thread_active_flags,
		if run.interactive_requested { "yes" } else { "no" },
		if run.continuation_pending { "yes" } else { "no" },
		continuation_recovery,
		phase_acceptance,
		branch_name,
		worktree_path,
		run.updated_at,
		run.last_run_activity_at.as_deref().unwrap_or("none"),
		run.last_protocol_activity_at.as_deref().unwrap_or("none"),
		run.last_progress_at.as_deref().unwrap_or("none"),
		idle_for_seconds,
		protocol_idle_for_seconds,
		if run.suspected_stall { "yes" } else { "no" },
		run.progress_diagnostic.as_deref().unwrap_or("none"),
		run.process_id.map_or_else(|| String::from("none"), |value| value.to_string()),
		render_optional_bool(run.process_alive),
		run.process_liveness_reason.as_deref().unwrap_or("none"),
		run.retry_kind.as_deref().unwrap_or("none"),
		run.next_retry_at.as_deref().unwrap_or("none"),
		run.effective_model.as_deref().unwrap_or("none"),
		run.effective_model_provider.as_deref().unwrap_or("none"),
		run.effective_cwd.as_deref().unwrap_or("none"),
		run.effective_approval_policy.as_deref().unwrap_or("none"),
		run.effective_approvals_reviewer.as_deref().unwrap_or("none"),
		run.effective_sandbox_mode.as_deref().unwrap_or("none"),
		protocol_event,
		run.event_count
	));
}

fn render_run_protocol_event(run: &OperatorRunStatus) -> String {
	match (&run.last_event_type, &run.last_event_at) {
		(Some(event_type), Some(timestamp)) => format!("{event_type} @ {timestamp}"),
		(Some(event_type), None) => event_type.clone(),
		(None, Some(timestamp)) => timestamp.clone(),
		(None, None) => String::from("none"),
	}
}

fn render_run_thread_active_flags(run: &OperatorRunStatus) -> String {
	if run.thread_active_flags.is_empty() {
		String::from("none")
	} else {
		run.thread_active_flags.join(",")
	}
}

fn render_lane_control_conditions(run: &OperatorRunStatus) -> String {
	if run.lane_control_conditions.is_empty() {
		String::from("none")
	} else {
		run.lane_control_conditions.join(",")
	}
}

fn render_continuation_recovery_summary(
	recovery: Option<&OperatorContinuationRecoveryStatus>,
) -> String {
	let Some(recovery) = recovery else {
		return String::from("none");
	};
	let message = recovery
		.source_error_message
		.as_deref()
		.map(single_line_status_value)
		.unwrap_or_else(|| String::from("none"));

	format!(
		"state={} source_phase={} next_phase={} source_error_class={} source_error_message={} count={}/{} budget_exceeded={} recorded_at={} run_id={} attempt={} next_action={}",
		recovery.state,
		recovery.source_phase,
		recovery.next_phase,
		recovery.source_error_class,
		message,
		recovery.recovery_count,
		recovery.automatic_continuation_limit,
		if recovery.budget_exceeded { "yes" } else { "no" },
		recovery.recorded_at,
		recovery.run_id,
		recovery.attempt_number,
		recovery.next_action,
	)
}

fn render_phase_acceptance_summary(acceptance: Option<&OperatorPhaseAcceptanceStatus>) -> String {
	let Some(acceptance) = acceptance else {
		return String::from("none");
	};
	let surfaces = if acceptance.changed_surfaces.is_empty() {
		String::from("none")
	} else {
		acceptance.changed_surfaces.join(",")
	};

	format!(
		"phase={} decision={} reason={} objective_covered={} effective_delta={} surfaces={} non_goal_passed={} validation_passed={} recorded_at={} run_id={} attempt={} next_action={}",
		acceptance.phase,
		acceptance.decision,
		acceptance.reason_code,
		if acceptance.objective_covered { "yes" } else { "no" },
		if acceptance.effective_delta_present { "yes" } else { "no" },
		surfaces,
		if acceptance.non_goal_passed { "yes" } else { "no" },
		if acceptance.validation_passed { "yes" } else { "no" },
		acceptance.recorded_at,
		acceptance.run_id,
		acceptance.attempt_number,
		acceptance.next_action
	)
}

fn single_line_status_value(value: &str) -> String {
	value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn render_optional_bool(value: Option<bool>) -> String {
	value.map_or_else(|| String::from("none"), |value| if value { "yes" } else { "no" }.into())
}

fn operator_run_queue_lease_summary(run: &OperatorRunStatus) -> String {
	if run.run_lease {
		return String::from("held");
	}

	match run.execution_liveness.as_str() {
		"process_alive" => String::from("not_held (process_alive keeps lane visible)"),
		"thread_active" => String::from("not_held (thread_active keeps lane visible)"),
		"protocol_observed" => String::from("not_held (protocol_observed keeps lane visible)"),
		"process_stopped" => String::from("not_held (process_stopped needs attention)"),
		EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH
			if orchestrator::operator_run_has_recent_app_server_execution(run) =>
			String::from("not_held (app_server_activity keeps lane visible)"),
		EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH =>
			String::from("not_held (process_identity_mismatch needs attention)"),
		_ => String::from("not_held"),
	}
}

fn operator_run_freshness(run: &OperatorRunStatus) -> (&'static str, &str) {
	if orchestrator::operator_run_counts_as_current_lane(run) {
		if let Some(timestamp) = run.last_run_activity_at.as_deref() {
			return ("last_run_activity_at", timestamp);
		}
		if let Some(timestamp) = run.last_progress_at.as_deref() {
			return ("last_progress_at", timestamp);
		}
		if let Some(timestamp) = run.last_protocol_activity_at.as_deref() {
			return ("last_protocol_activity_at", timestamp);
		}

		return ("none", "none");
	}

	("updated_at", run.updated_at.as_str())
}
