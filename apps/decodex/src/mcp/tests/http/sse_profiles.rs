use serde_json::Value;

use crate::mcp::{McpCapabilityProfile, tests::support};

#[test]
fn streamable_http_sse_response_includes_progress_notification() {
	let repo = support::test_repo();
	let mut handler = support::http_handler(repo.path(), McpCapabilityProfile::Admin);
	let initialize = support::run_http(
		&mut handler,
		support::http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193")],
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
		),
	);
	let session_id = initialize.header("mcp-session-id").expect("session id").to_owned();
	let response = support::run_http(
		&mut handler,
		support::http_post(
			"/mcp",
			[
				("Origin", "http://127.0.0.1:8193"),
				("Accept", "text/event-stream"),
				("Mcp-Session-Id", session_id.as_str()),
			],
			r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"},"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}"#,
		),
	);
	let body = response.body_text();

	assert_eq!(response.status, "HTTP/1.1 200 OK");
	assert_eq!(response.header("content-type"), Some("text/event-stream"));
	assert_eq!(response.header("access-control-allow-origin"), Some("http://127.0.0.1:8193"));
	assert_eq!(response.header("access-control-expose-headers"), Some("Mcp-Session-Id"));
	assert!(body.contains("event: message"));
	assert!(body.contains("\"method\":\"notifications/progress\""));
	assert!(body.contains("\"id\":2"));
}

#[test]
fn streamable_http_observe_profile_exposes_only_observe_tool() {
	let repo = support::test_repo();
	let mut handler = support::http_handler(repo.path(), McpCapabilityProfile::Observe);
	let initialize = support::run_http(
		&mut handler,
		support::http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193")],
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
		),
	);
	let session_id = initialize.header("mcp-session-id").expect("session id").to_owned();
	let response = support::run_http(
		&mut handler,
		support::http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193"), ("Mcp-Session-Id", session_id.as_str())],
			r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
		),
	);
	let body = response.json_body();
	let tool_names = body["result"]["tools"]
		.as_array()
		.expect("tools array")
		.iter()
		.filter_map(|tool| tool.get("name").and_then(Value::as_str))
		.collect::<Vec<_>>();

	assert_eq!(tool_names, vec!["decodex_observe"]);
}

#[test]
fn streamable_http_observe_profile_refuses_operate_and_admin_calls() {
	let repo = support::test_repo();
	let mut handler = support::http_handler(repo.path(), McpCapabilityProfile::Observe);
	let initialize = support::run_http(
		&mut handler,
		support::http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193")],
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
		),
	);
	let session_id = initialize.header("mcp-session-id").expect("session id").to_owned();

	for (tool, required_profile, arguments) in [
		("decodex_lane_control", "operate", r#"{"action":"inspect"}"#),
		("decodex_project_control", "admin", r#"{"action":"status","projectId":"pubfi"}"#),
	] {
		let response = support::run_http(
			&mut handler,
			support::http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193"), ("Mcp-Session-Id", session_id.as_str())],
				&format!(
					r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{tool}","arguments":{arguments}}}}}"#
				),
			),
		);
		let body = response.json_body();
		let structured = &body["result"]["structuredContent"];

		assert_eq!(structured["schema"], "decodex.mcp.refusal/1");
		assert_eq!(structured["reason"], "insufficient_capability_profile");
		assert_eq!(structured["capability_profile"], "observe");
		assert_eq!(structured["required_capability_profile"], required_profile);
	}
}
