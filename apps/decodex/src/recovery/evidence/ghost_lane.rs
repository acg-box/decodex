use crate::{
	recovery::{
		GHOST_LANE_CLASSIFICATION, GHOST_LANE_CLEANUP_EVENT, GHOST_LANE_TERMINAL_STATUS,
		MCP_TEST_FIXTURE_ALT_ISSUE_IDENTIFIER, MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION,
		MCP_TEST_FIXTURE_ISSUE_ID, MCP_TEST_FIXTURE_PROJECT_ID, MCP_TEST_FIXTURE_RUN_ID,
		MCP_TEST_FIXTURE_SOURCE, MCP_TEST_FIXTURE_THREAD_ID, MCP_TEST_FIXTURE_TURN_ID,
	},
	state::{PrivateExecutionEvent, ProjectRunStatus},
	tracker::records::LinearExecutionEventRecord,
};

pub(in crate::recovery) fn ghost_lane_has_mcp_test_fixture_identity(
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

pub(in crate::recovery) fn ghost_lane_private_events_are_mcp_test_recovery_evidence(
	events: &[PrivateExecutionEvent],
) -> bool {
	!events.is_empty()
		&& events.iter().all(|event| {
			ghost_lane_private_event_is_mcp_test_control_evidence(event)
				|| ghost_lane_private_event_is_cleanup_audit(event)
		})
}

pub(in crate::recovery) fn ghost_lane_private_events_are_cleanup_audit_evidence(
	events: &[PrivateExecutionEvent],
) -> bool {
	!events.is_empty() && events.iter().all(ghost_lane_private_event_is_cleanup_audit)
}

pub(in crate::recovery) fn ghost_lane_private_event_is_cleanup_audit(
	event: &PrivateExecutionEvent,
) -> bool {
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

pub(in crate::recovery) fn ghost_lane_mcp_test_fixture_allowed_live_blocker(blocker: &str) -> bool {
	matches!(
		blocker,
		"protocol_event_evidence_present"
			| "protocol_activity_present"
			| "thread_reference_present"
	)
}

pub(in crate::recovery) fn ghost_lane_record_has_pr_or_review_lineage(
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

fn ghost_lane_mcp_test_fixture_issue_identifier_matches(issue_identifier: Option<&str>) -> bool {
	match issue_identifier {
		Some(value) =>
			value == MCP_TEST_FIXTURE_ISSUE_ID || value == MCP_TEST_FIXTURE_ALT_ISSUE_IDENTIFIER,
		None => true,
	}
}

fn ghost_lane_optional_fixture_value(value: Option<&str>, expected: &str) -> bool {
	match value {
		Some(value) => value == expected,
		None => true,
	}
}

fn ghost_lane_private_event_is_mcp_test_control_evidence(event: &PrivateExecutionEvent) -> bool {
	match event.event_type() {
		"control_action" =>
			ghost_lane_private_event_source(event.payload()) == Some(MCP_TEST_FIXTURE_SOURCE)
				|| ghost_lane_cli_control_action_matches_mcp_test_fixture(event.payload()),
		"lane_control/steer/requested" | "lane_control/interrupt/requested" =>
			ghost_lane_private_event_source(event.payload()) == Some(MCP_TEST_FIXTURE_SOURCE),
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

fn ghost_lane_cleanup_audit_evidence_contains(payload: &serde_json::Value, expected: &str) -> bool {
	payload
		.get("evidence")
		.and_then(serde_json::Value::as_array)
		.is_some_and(|evidence| evidence.iter().any(|entry| entry.as_str() == Some(expected)))
}
