use crate::{
	config::ServiceConfig,
	orchestrator::{GHOST_LANE_TERMINAL_STATUS, OperatorRunStatus},
	prelude::Result,
	state::{PrivateExecutionEvent, StateStore},
};

const CLEANUP_EVENT: &str = "ghost_lane_cleanup";
const MCP_TEST_FIXTURE_SOURCE: &str = "mcp-test";
const MCP_TEST_FIXTURE_PROJECT_ID: &str = "pubfi";
const MCP_TEST_FIXTURE_ISSUE_ID: &str = "PUB-012";
const MCP_TEST_FIXTURE_ALT_ISSUE_IDENTIFIER: &str = "PUBFI-012";
const MCP_TEST_FIXTURE_RUN_ID: &str = "run-12";
const MCP_TEST_FIXTURE_THREAD_ID: &str = "thread-12";
const MCP_TEST_FIXTURE_TURN_ID: &str = "turn-12";

pub(in crate::orchestrator) fn mcp_test_fixture_control_evidence(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &OperatorRunStatus,
) -> Result<bool> {
	if !has_mcp_test_fixture_identity(project, run) {
		return Ok(false);
	}

	let events = state_store.list_private_execution_events(
		project.service_id(),
		&run.issue_id,
		&run.run_id,
		run.attempt_number,
	)?;

	Ok(private_events_are_mcp_test_recovery_evidence(&events))
}

pub(in crate::orchestrator) fn private_events_are_cleanup_audit_evidence(
	events: &[PrivateExecutionEvent],
) -> bool {
	!events.is_empty() && events.iter().all(private_event_is_cleanup_audit)
}

pub(in crate::orchestrator) fn private_event_is_cleanup_audit(
	event: &PrivateExecutionEvent,
) -> bool {
	if event.event_type() != CLEANUP_EVENT {
		return false;
	}

	let payload = event.payload();

	payload.get("schema").and_then(serde_json::Value::as_str)
		== Some("decodex.ghost_lane_recovery_private_event/1")
		&& payload.get("event").and_then(serde_json::Value::as_str) == Some(CLEANUP_EVENT)
		&& matches!(
			payload.get("classification").and_then(serde_json::Value::as_str),
			Some("missing_issue_ghost_lane" | "mcp_test_fixture_ghost_lane")
		) && payload.get("terminal_status").and_then(serde_json::Value::as_str)
		== Some(GHOST_LANE_TERMINAL_STATUS)
		&& payload.get("cleared_run_lease").and_then(serde_json::Value::as_bool) == Some(true)
		&& payload
			.get("blockers")
			.and_then(serde_json::Value::as_array)
			.is_some_and(|blockers| blockers.is_empty())
		&& cleanup_audit_evidence_contains(payload, "tracker_issue_missing")
		&& cleanup_audit_evidence_contains(payload, "worktree_missing")
		&& cleanup_audit_evidence_contains(payload, "review_lineage_missing")
}

pub(in crate::orchestrator) fn mcp_test_fixture_allowed_live_blocker(blocker: &str) -> bool {
	matches!(
		blocker,
		"protocol_event_evidence_present"
			| "protocol_activity_present"
			| "thread_reference_present"
	)
}

fn has_mcp_test_fixture_identity(project: &ServiceConfig, run: &OperatorRunStatus) -> bool {
	project.service_id() == MCP_TEST_FIXTURE_PROJECT_ID
		&& run.issue_id == MCP_TEST_FIXTURE_ISSUE_ID
		&& run.run_id == MCP_TEST_FIXTURE_RUN_ID
		&& run.attempt_number == 1
		&& mcp_test_fixture_issue_identifier_matches(run.issue_identifier.as_deref())
		&& optional_fixture_value(run.thread_id.as_deref(), MCP_TEST_FIXTURE_THREAD_ID)
		&& optional_fixture_value(run.turn_id.as_deref(), MCP_TEST_FIXTURE_TURN_ID)
}

fn mcp_test_fixture_issue_identifier_matches(issue_identifier: Option<&str>) -> bool {
	match issue_identifier {
		Some(value) => {
			value == MCP_TEST_FIXTURE_ISSUE_ID || value == MCP_TEST_FIXTURE_ALT_ISSUE_IDENTIFIER
		},
		None => true,
	}
}

fn optional_fixture_value(value: Option<&str>, expected: &str) -> bool {
	match value {
		Some(value) => value == expected,
		None => true,
	}
}

fn private_events_are_mcp_test_recovery_evidence(events: &[PrivateExecutionEvent]) -> bool {
	!events.is_empty()
		&& events.iter().all(|event| {
			private_event_is_mcp_test_control_evidence(event)
				|| private_event_is_cleanup_audit(event)
		})
}

fn private_event_is_mcp_test_control_evidence(event: &PrivateExecutionEvent) -> bool {
	match event.event_type() {
		"control_action" => {
			private_event_source(event.payload()) == Some(MCP_TEST_FIXTURE_SOURCE)
				|| cli_control_action_matches_mcp_test_fixture(event.payload())
		},
		"lane_control/steer/requested" | "lane_control/interrupt/requested" => {
			private_event_source(event.payload()) == Some(MCP_TEST_FIXTURE_SOURCE)
		},
		_ => false,
	}
}

fn private_event_source(payload: &serde_json::Value) -> Option<&str> {
	payload
		.get("source")
		.and_then(serde_json::Value::as_str)
		.or_else(|| payload.pointer("/authority/source").and_then(serde_json::Value::as_str))
}

fn cli_control_action_matches_mcp_test_fixture(payload: &serde_json::Value) -> bool {
	private_event_source(payload) == Some("cli")
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

fn cleanup_audit_evidence_contains(payload: &serde_json::Value, expected: &str) -> bool {
	payload
		.get("evidence")
		.and_then(serde_json::Value::as_array)
		.is_some_and(|evidence| evidence.iter().any(|entry| entry.as_str() == Some(expected)))
}
