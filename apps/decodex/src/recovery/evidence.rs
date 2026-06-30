//! Evidence predicates for explicit operator recovery diagnostics.

use crate::{
	state::{PrivateExecutionEvent, ProjectRunStatus},
	tracker::records::LinearExecutionEventRecord,
};

use super::{
	GHOST_LANE_CLASSIFICATION, GHOST_LANE_CLEANUP_EVENT, GHOST_LANE_TERMINAL_STATUS,
	MCP_TEST_FIXTURE_ALT_ISSUE_IDENTIFIER, MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION,
	MCP_TEST_FIXTURE_ISSUE_ID, MCP_TEST_FIXTURE_PROJECT_ID, MCP_TEST_FIXTURE_RUN_ID,
	MCP_TEST_FIXTURE_SOURCE, MCP_TEST_FIXTURE_THREAD_ID, MCP_TEST_FIXTURE_TURN_ID,
	STALE_ACTIVE_RECOVERY_SCHEMA, STALE_ACTIVE_RELEASE_EVENT,
	process_liveness::StaleActiveProcessLiveness,
};

pub(super) fn stale_active_private_event_allows_release(
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

pub(super) fn stale_active_private_event_is_release_audit_for_run(
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
		"lane_control/interrupt/requested" => {
			event.payload().get("method").and_then(serde_json::Value::as_str)
				== Some("turn/interrupt")
		},
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
		&& json_string_is_missing_or_empty(payload.get("pr_url"))
		&& json_array_is_missing_or_empty(payload.get("verification"))
}

fn stale_active_private_event_is_no_diff_guardrail(event: &PrivateExecutionEvent) -> bool {
	if event.event_type() != "loop_guardrail_checkpoint" {
		return false;
	}
	let payload = event.payload();

	payload.get("schema").and_then(serde_json::Value::as_str)
		== Some("decodex.loop_guardrail_checkpoint/1")
		&& payload.get("source_error_class").and_then(serde_json::Value::as_str)
			== Some("app_server_turn_failed")
		&& payload.get("reason").and_then(serde_json::Value::as_str) == Some("no_effective_diff")
		&& stale_active_guardrail_details_have_no_delta(payload)
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

	json_bool_is_false(details.get("branch_delta_present"))
		&& json_bool_is_false(details.get("effective_delta_present"))
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
		&& json_array_is_missing_or_empty(payload.get("contracts"))
		&& json_array_is_missing_or_empty(payload.get("execution_programs"))
		&& stale_active_harness_pr_lifecycle_has_no_progress(payload)
		&& stale_active_harness_review_has_no_progress(payload)
		&& stale_active_harness_validation_has_no_progress(payload)
}

fn stale_active_harness_pr_lifecycle_has_no_progress(payload: &serde_json::Value) -> bool {
	json_array_is_missing_or_empty(payload.pointer("/pr_lifecycle/pr_urls"))
}

fn stale_active_harness_review_has_no_progress(payload: &serde_json::Value) -> bool {
	let review = payload.pointer("/review");
	let statuses = review.and_then(|review| review.get("statuses"));
	let accepted_findings = review.and_then(|review| review.get("accepted_finding_count"));
	let rejected_findings = review.and_then(|review| review.get("rejected_finding_count"));
	let nonclean_rounds = review.and_then(|review| review.get("nonclean_rounds"));

	json_array_is_missing_or_empty(statuses)
		&& json_number_is_zero_or_missing(accepted_findings)
		&& json_number_is_zero_or_missing(rejected_findings)
		&& json_number_is_zero_or_missing(nonclean_rounds)
}

fn stale_active_harness_validation_has_no_progress(payload: &serde_json::Value) -> bool {
	let validation = payload.pointer("/validation");
	let validation_result = validation
		.and_then(|validation| validation.get("result"))
		.and_then(serde_json::Value::as_str);
	let failure_count = validation.and_then(|validation| validation.get("failure_count"));
	let failure_classes = validation.and_then(|validation| validation.get("failure_classes"));

	validation_result.is_none_or(|result| result == "not_recorded")
		&& json_number_is_zero_or_missing(failure_count)
		&& json_array_is_missing_or_empty(failure_classes)
}

fn json_bool_is_false(value: Option<&serde_json::Value>) -> bool {
	value.and_then(serde_json::Value::as_bool) == Some(false)
}

fn json_number_is_zero_or_missing(value: Option<&serde_json::Value>) -> bool {
	value.is_none_or(|value| value.as_u64() == Some(0))
}

fn json_string_is_missing_or_empty(value: Option<&serde_json::Value>) -> bool {
	value.is_none_or(|value| value.as_str().is_none_or(|value| value.is_empty()) || value.is_null())
}

fn json_array_is_missing_or_empty(value: Option<&serde_json::Value>) -> bool {
	value.is_none_or(|value| value.as_array().is_none_or(Vec::is_empty))
}

pub(super) fn ghost_lane_has_mcp_test_fixture_identity(
	project_id: &str,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
) -> bool {
	project_id == MCP_TEST_FIXTURE_PROJECT_ID
		&& run.issue_id() == MCP_TEST_FIXTURE_ISSUE_ID
		&& run.run_id() == MCP_TEST_FIXTURE_RUN_ID
		&& run.attempt_number() == 1
		&& ghost_lane_mcp_test_fixture_issue_identifier_matches(issue_identifier)
		&& ghost_lane_optional_fixture_value(run.thread_id(), MCP_TEST_FIXTURE_THREAD_ID)
		&& ghost_lane_optional_fixture_value(run.turn_id(), MCP_TEST_FIXTURE_TURN_ID)
}

fn ghost_lane_mcp_test_fixture_issue_identifier_matches(issue_identifier: Option<&str>) -> bool {
	match issue_identifier {
		Some(value) => {
			value == MCP_TEST_FIXTURE_ISSUE_ID || value == MCP_TEST_FIXTURE_ALT_ISSUE_IDENTIFIER
		},
		None => true,
	}
}

fn ghost_lane_optional_fixture_value(value: Option<&str>, expected: &str) -> bool {
	match value {
		Some(value) => value == expected,
		None => true,
	}
}

pub(super) fn ghost_lane_private_events_are_mcp_test_recovery_evidence(
	events: &[PrivateExecutionEvent],
) -> bool {
	!events.is_empty()
		&& events.iter().all(|event| {
			ghost_lane_private_event_is_mcp_test_control_evidence(event)
				|| ghost_lane_private_event_is_cleanup_audit(event)
		})
}

fn ghost_lane_private_event_is_mcp_test_control_evidence(event: &PrivateExecutionEvent) -> bool {
	match event.event_type() {
		"control_action" => {
			ghost_lane_private_event_source(event.payload()) == Some(MCP_TEST_FIXTURE_SOURCE)
				|| ghost_lane_cli_control_action_matches_mcp_test_fixture(event.payload())
		},
		"lane_control/steer/requested" | "lane_control/interrupt/requested" => {
			ghost_lane_private_event_source(event.payload()) == Some(MCP_TEST_FIXTURE_SOURCE)
		},
		_ => false,
	}
}

fn ghost_lane_private_event_source(payload: &serde_json::Value) -> Option<&str> {
	payload
		.get("source")
		.and_then(serde_json::Value::as_str)
		.or_else(|| payload.pointer("/authority/source").and_then(serde_json::Value::as_str))
}

fn ghost_lane_cli_control_action_matches_mcp_test_fixture(payload: &serde_json::Value) -> bool {
	ghost_lane_private_event_source(payload) == Some("cli")
		&& matches!(
			payload.get("action").and_then(serde_json::Value::as_str),
			Some("steer" | "interrupt")
		) && payload.pointer("/requested/project_id").and_then(serde_json::Value::as_str)
		== Some(MCP_TEST_FIXTURE_PROJECT_ID)
		&& payload.pointer("/requested/issue_id").and_then(serde_json::Value::as_str)
			== Some(MCP_TEST_FIXTURE_ISSUE_ID)
		&& payload.pointer("/requested/run_id").and_then(serde_json::Value::as_str)
			== Some(MCP_TEST_FIXTURE_RUN_ID)
		&& payload.pointer("/requested/attempt_number").and_then(serde_json::Value::as_i64)
			== Some(1)
}

pub(super) fn ghost_lane_private_events_are_cleanup_audit_evidence(
	events: &[PrivateExecutionEvent],
) -> bool {
	!events.is_empty() && events.iter().all(ghost_lane_private_event_is_cleanup_audit)
}

pub(super) fn ghost_lane_private_event_is_cleanup_audit(event: &PrivateExecutionEvent) -> bool {
	if event.event_type() != GHOST_LANE_CLEANUP_EVENT {
		return false;
	}

	let payload = event.payload();

	payload.get("schema").and_then(serde_json::Value::as_str)
		== Some("decodex.ghost_lane_recovery_private_event/1")
		&& payload.get("event").and_then(serde_json::Value::as_str)
			== Some(GHOST_LANE_CLEANUP_EVENT)
		&& matches!(
			payload.get("classification").and_then(serde_json::Value::as_str),
			Some(GHOST_LANE_CLASSIFICATION | MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION)
		) && payload.get("terminal_status").and_then(serde_json::Value::as_str)
		== Some(GHOST_LANE_TERMINAL_STATUS)
		&& payload.get("cleared_run_lease").and_then(serde_json::Value::as_bool) == Some(true)
		&& payload
			.get("blockers")
			.and_then(serde_json::Value::as_array)
			.is_some_and(|blockers| blockers.is_empty())
		&& ghost_lane_cleanup_audit_evidence_contains(payload, "tracker_issue_missing")
		&& ghost_lane_cleanup_audit_evidence_contains(payload, "worktree_missing")
		&& ghost_lane_cleanup_audit_evidence_contains(payload, "review_lineage_missing")
}

fn ghost_lane_cleanup_audit_evidence_contains(payload: &serde_json::Value, expected: &str) -> bool {
	payload
		.get("evidence")
		.and_then(serde_json::Value::as_array)
		.is_some_and(|evidence| evidence.iter().any(|entry| entry.as_str() == Some(expected)))
}

pub(super) fn ghost_lane_mcp_test_fixture_allowed_live_blocker(blocker: &str) -> bool {
	matches!(
		blocker,
		"protocol_event_evidence_present"
			| "protocol_activity_present"
			| "thread_reference_present"
	)
}

pub(super) fn ghost_lane_record_has_pr_or_review_lineage(
	record: &LinearExecutionEventRecord,
) -> bool {
	record.pr_url.as_ref().is_some_and(|value| !value.trim().is_empty())
		|| record.pr_head_sha.as_ref().is_some_and(|value| !value.trim().is_empty())
		|| record.pr_base_ref.as_ref().is_some_and(|value| !value.trim().is_empty())
		|| matches!(
			record.event_type.as_str(),
			"review_handoff"
				| "review_handoff_rebind"
				| "review_handoff_adopt"
				| "review_repair"
				| "landed" | "closeout"
				| "cleanup_complete"
		) || record.terminal_path.as_deref() == Some("review_handoff")
}
