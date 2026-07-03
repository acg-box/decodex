use crate::{
	recovery::{
		STALE_ACTIVE_RECOVERY_SCHEMA, STALE_ACTIVE_RELEASE_EVENT, evidence::json,
		process_liveness::StaleActiveProcessLiveness,
	},
	state::{PrivateExecutionEvent, ProjectRunStatus},
};

pub(in crate::recovery) fn stale_active_private_event_allows_release(
	event: &PrivateExecutionEvent,
	marker_liveness: StaleActiveProcessLiveness,
	release_audit_present: bool,
) -> bool {
	stale_active_private_event_is_release_audit(event)
		|| stale_active_private_event_is_failed_control_attempt(event)
		|| ((marker_liveness == StaleActiveProcessLiveness::NotAlive || release_audit_present)
			&& stale_active_private_event_is_dead_process_control_telemetry(event))
		|| stale_active_private_event_is_stale_runtime_marker(event)
		|| stale_active_private_event_is_probing_checkpoint(event)
		|| stale_active_private_event_is_no_diff_guardrail(event)
		|| stale_active_private_event_is_phase_goal_runtime_failure_telemetry(event)
		|| stale_active_private_event_is_no_progress_harness_outcome(event)
}

pub(in crate::recovery) fn stale_active_private_event_is_release_audit_for_run(
	event: &PrivateExecutionEvent,
	run: Option<&ProjectRunStatus>,
) -> bool {
	let Some(run) = run else {
		return false;
	};

	stale_active_private_event_is_release_audit(event)
		&& event.run_id() == run.run_id()
		&& event.attempt_number() == run.attempt_number()
}

fn stale_active_private_event_is_release_audit(event: &PrivateExecutionEvent) -> bool {
	event.event_type() == STALE_ACTIVE_RELEASE_EVENT
		&& event.payload().get("schema").and_then(serde_json::Value::as_str)
			== Some(STALE_ACTIVE_RECOVERY_SCHEMA)
		&& event.payload().get("event").and_then(serde_json::Value::as_str)
			== Some(STALE_ACTIVE_RELEASE_EVENT)
}

fn stale_active_private_event_is_failed_control_attempt(event: &PrivateExecutionEvent) -> bool {
	if event.event_type() == "lane_control/interrupt" {
		return event.payload().get("processAliveAfter").and_then(serde_json::Value::as_bool)
			== Some(false)
			&& event.payload().get("status").and_then(serde_json::Value::as_str) == Some("sent");
	}

	event.event_type() == "control_action"
		&& matches!(
			event.payload().get("action").and_then(serde_json::Value::as_str),
			Some("interrupt" | "steer")
		) && matches!(
		event.payload().get("reason").and_then(serde_json::Value::as_str),
		Some(
			"run_lease_missing"
				| "hard_fallback_unavailable"
				| "hard_interrupt_fallback"
				| "process_not_signalable"
		)
	)
}

fn stale_active_private_event_is_dead_process_control_telemetry(
	event: &PrivateExecutionEvent,
) -> bool {
	match event.event_type() {
		"lane_control/interrupt/requested" =>
			event.payload().get("method").and_then(serde_json::Value::as_str)
				== Some("turn/interrupt"),
		"control_action" => {
			let payload = event.payload();

			payload.get("schema").and_then(serde_json::Value::as_str)
				== Some("decodex.run_control_action/v1")
				&& payload.get("action").and_then(serde_json::Value::as_str) == Some("interrupt")
				&& matches!(
					payload.get("reason").and_then(serde_json::Value::as_str),
					Some(
						"run_lease_control_channel_resolved"
							| "soft_interrupt_response_pending"
							| "hard_interrupt_fallback"
					)
				) && matches!(
				payload.get("outcome").and_then(serde_json::Value::as_str),
				Some("accepted" | "timed_out" | "fallback")
			) && payload.pointer("/context/process_alive").and_then(serde_json::Value::as_bool)
				== Some(false)
		},
		_ => false,
	}
}

fn stale_active_private_event_is_stale_runtime_marker(event: &PrivateExecutionEvent) -> bool {
	matches!(event.event_type(), "control_channel_published" | "phase_goal_set")
}

fn stale_active_private_event_is_probing_checkpoint(event: &PrivateExecutionEvent) -> bool {
	if event.event_type() != "progress_checkpoint" {
		return false;
	}

	let payload = event.payload();

	payload.get("phase").and_then(serde_json::Value::as_str) == Some("probing")
		&& json::string_is_missing_or_empty(payload.get("pr_url"))
		&& json::array_is_missing_or_empty(payload.get("verification"))
}

fn stale_active_private_event_is_no_diff_guardrail(event: &PrivateExecutionEvent) -> bool {
	if event.event_type() != "loop_guardrail_checkpoint" {
		return false;
	}

	let payload = event.payload();

	payload.get("schema").and_then(serde_json::Value::as_str)
		== Some("decodex.loop_guardrail_checkpoint/1")
		&& stale_active_no_diff_guardrail_source_is_startup_or_turn_failure(payload)
		&& payload.get("reason").and_then(serde_json::Value::as_str) == Some("no_effective_diff")
		&& stale_active_guardrail_details_have_no_delta(payload)
}

fn stale_active_no_diff_guardrail_source_is_startup_or_turn_failure(
	payload: &serde_json::Value,
) -> bool {
	let source_error_class = payload.get("source_error_class").and_then(serde_json::Value::as_str);

	matches!(source_error_class, Some("app_server_turn_failed") | None)
}

fn stale_active_private_event_is_phase_goal_runtime_failure_telemetry(
	event: &PrivateExecutionEvent,
) -> bool {
	if !matches!(event.event_type(), "phase_goal_recovery" | "phase_goal_recovery_blocked") {
		return false;
	}

	let payload = event.payload();
	let source_error_class =
		payload.pointer("/payload/sourceErrorClass").and_then(serde_json::Value::as_str);

	payload.get("schema").and_then(serde_json::Value::as_str) == Some("decodex.phase_goal_signal/1")
		&& payload.get("phase").and_then(serde_json::Value::as_str)
			== Some("implement_to_validation_ready")
		&& matches!(
			payload.get("signal").and_then(serde_json::Value::as_str),
			Some("phase_goal_recovered" | "continuation_budget_exhausted")
		) && source_error_class.is_some_and(|error_class| error_class.starts_with("app_server_"))
}

fn stale_active_guardrail_details_have_no_delta(payload: &serde_json::Value) -> bool {
	let details = payload
		.get("details")
		.and_then(serde_json::Value::as_str)
		.and_then(|details| serde_json::from_str::<serde_json::Value>(details).ok());
	let details = details.as_ref().unwrap_or(payload);

	json::bool_is_false(details.get("branch_delta_present"))
		&& json::bool_is_false(details.get("effective_delta_present"))
}

fn stale_active_private_event_is_no_progress_harness_outcome(
	event: &PrivateExecutionEvent,
) -> bool {
	if event.event_type() != "harness_outcome" {
		return false;
	}

	let payload = event.payload();

	payload.get("schema").and_then(serde_json::Value::as_str) == Some("decodex.harness_outcome/1")
		&& payload.pointer("/source/outcome").and_then(serde_json::Value::as_str)
			== Some("retryable_failure")
		&& payload.pointer("/pr_lifecycle/outcome").and_then(serde_json::Value::as_str)
			== Some("retryable_failure")
		&& payload.pointer("/manual_attention").is_none_or(serde_json::Value::is_null)
		&& json::array_is_missing_or_empty(payload.get("contracts"))
		&& json::array_is_missing_or_empty(payload.get("execution_programs"))
		&& stale_active_harness_pr_lifecycle_has_no_progress(payload)
		&& stale_active_harness_review_has_no_progress(payload)
		&& stale_active_harness_validation_has_no_progress(payload)
}

fn stale_active_harness_pr_lifecycle_has_no_progress(payload: &serde_json::Value) -> bool {
	json::array_is_missing_or_empty(payload.pointer("/pr_lifecycle/pr_urls"))
}

fn stale_active_harness_review_has_no_progress(payload: &serde_json::Value) -> bool {
	let review = payload.pointer("/review");
	let statuses = review.and_then(|review| review.get("statuses"));
	let accepted_findings = review.and_then(|review| review.get("accepted_finding_count"));
	let rejected_findings = review.and_then(|review| review.get("rejected_finding_count"));
	let nonclean_rounds = review.and_then(|review| review.get("nonclean_rounds"));

	json::array_is_missing_or_empty(statuses)
		&& json::number_is_zero_or_missing(accepted_findings)
		&& json::number_is_zero_or_missing(rejected_findings)
		&& json::number_is_zero_or_missing(nonclean_rounds)
}

fn stale_active_harness_validation_has_no_progress(payload: &serde_json::Value) -> bool {
	let validation = payload.pointer("/validation");
	let validation_result = validation
		.and_then(|validation| validation.get("result"))
		.and_then(serde_json::Value::as_str);
	let failure_count = validation.and_then(|validation| validation.get("failure_count"));
	let failure_classes = validation.and_then(|validation| validation.get("failure_classes"));

	validation_result.is_none_or(|result| result == "not_recorded")
		&& json::number_is_zero_or_missing(failure_count)
		&& json::array_is_missing_or_empty(failure_classes)
}
