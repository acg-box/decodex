use serde_json::Value;

use crate::mcp::{
	self, DEFAULT_MCP_HTTP_LISTEN_ADDRESS, McpCapabilityProfile, McpHttpAuthorization,
};

use super::support::{
	http_delete, http_handler, http_handler_with_allowed_origins, http_handler_with_authorization,
	http_options, http_post, run_http, test_repo,
};

#[test]
fn streamable_http_json_post_initializes_session() {
	let repo = test_repo();
	let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);
	let response = run_http(
		&mut handler,
		http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193"), ("Accept", "application/json")],
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
		),
	);
	let body = response.json_body();

	assert_eq!(response.status, "HTTP/1.1 200 OK");
	assert_eq!(response.header("content-type"), Some("application/json"));
	assert!(response.header("mcp-session-id").is_some());
	assert_eq!(response.header("access-control-allow-origin"), Some("http://127.0.0.1:8193"));
	assert_eq!(response.header("access-control-expose-headers"), Some("Mcp-Session-Id"));
	assert_eq!(
		body["result"]["capabilities"]["experimental"]["decodex"]["transport"],
		"streamable-http"
	);
	assert_eq!(
		body["result"]["capabilities"]["experimental"]["decodex"]["remoteControl"]["httpTransport"],
		"streamable-http"
	);
}

#[test]
fn streamable_http_allows_cors_preflight_for_trusted_origin() {
	let repo = test_repo();
	let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);
	let response = run_http(
		&mut handler,
		http_options(
			"/mcp",
			[
				("Origin", "http://127.0.0.1:8193"),
				("Access-Control-Request-Method", "POST"),
				("Access-Control-Request-Headers", "Content-Type, Mcp-Session-Id"),
			],
		),
	);

	assert_eq!(response.status, "HTTP/1.1 204 No Content");
	assert_eq!(response.header("access-control-allow-origin"), Some("http://127.0.0.1:8193"));
	assert_eq!(response.header("access-control-allow-methods"), Some("POST, DELETE, OPTIONS"));
	assert_eq!(
		response.header("access-control-allow-headers"),
		Some("Content-Type, Accept, Mcp-Session-Id, Authorization")
	);
}

#[test]
fn streamable_http_bearer_auth_challenges_missing_or_invalid_authorization() {
	let repo = test_repo();
	let mut handler = http_handler_with_authorization(
		repo.path(),
		McpCapabilityProfile::Observe,
		McpHttpAuthorization::from_token_for_test("secret-token"),
	);

	for headers in [
		vec![("Origin", "http://127.0.0.1:8193")],
		vec![("Origin", "http://127.0.0.1:8193"), ("Authorization", "Bearer wrong-token")],
	] {
		let response = run_http(
			&mut handler,
			http_post(
				"/mcp",
				headers,
				r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
			),
		);
		let body = response.json_body();

		assert_eq!(response.status, "HTTP/1.1 401 Unauthorized");
		assert_eq!(response.header("www-authenticate"), Some("Bearer realm=\"decodex-mcp\""));
		assert_eq!(body["error"]["message"], "Unauthorized");
		assert!(!response.body_text().contains("secret-token"));
	}
}

#[test]
fn streamable_http_bearer_auth_accepts_valid_authorization() {
	let repo = test_repo();
	let mut handler = http_handler_with_authorization(
		repo.path(),
		McpCapabilityProfile::Observe,
		McpHttpAuthorization::from_token_for_test("secret-token"),
	);
	let preflight = run_http(
		&mut handler,
		http_options(
			"/mcp",
			[
				("Origin", "http://127.0.0.1:8193"),
				("Access-Control-Request-Method", "POST"),
				("Access-Control-Request-Headers", "Authorization, Content-Type"),
			],
		),
	);
	let response = run_http(
		&mut handler,
		http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193"), ("Authorization", "Bearer secret-token")],
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
		),
	);
	let body = response.json_body();

	assert_eq!(preflight.status, "HTTP/1.1 204 No Content");
	assert_eq!(
		preflight.header("access-control-allow-headers"),
		Some("Content-Type, Accept, Mcp-Session-Id, Authorization")
	);
	assert_eq!(response.status, "HTTP/1.1 200 OK");
	assert!(response.header("mcp-session-id").is_some());
	assert_eq!(
		body["result"]["capabilities"]["experimental"]["decodex"]["capabilityProfile"],
		"observe"
	);
	assert!(!response.body_text().contains("secret-token"));
}

#[test]
fn streamable_http_allows_configured_origin() {
	let repo = test_repo();
	let mut handler = http_handler_with_allowed_origins(
		repo.path(),
		McpCapabilityProfile::Admin,
		vec![String::from("https://relay.example")],
	);
	let preflight = run_http(
		&mut handler,
		http_options(
			"/mcp",
			[("Origin", "https://relay.example"), ("Access-Control-Request-Method", "POST")],
		),
	);
	let initialize = run_http(
		&mut handler,
		http_post(
			"/mcp",
			[("Origin", "https://relay.example")],
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
		),
	);

	assert_eq!(preflight.status, "HTTP/1.1 204 No Content");
	assert_eq!(preflight.header("access-control-allow-origin"), Some("https://relay.example"));
	assert_eq!(initialize.status, "HTTP/1.1 200 OK");
	assert!(initialize.header("mcp-session-id").is_some());
	assert_eq!(initialize.header("access-control-allow-origin"), Some("https://relay.example"));
}

#[test]
fn streamable_http_sse_response_includes_progress_notification() {
	let repo = test_repo();
	let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);
	let initialize = run_http(
		&mut handler,
		http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193")],
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
		),
	);
	let session_id = initialize.header("mcp-session-id").expect("session id").to_owned();
	let response = run_http(
		&mut handler,
		http_post(
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
fn streamable_http_initialize_notification_does_not_create_session() {
	let repo = test_repo();
	let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);
	let response = run_http(
		&mut handler,
		http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193")],
			r#"{"jsonrpc":"2.0","method":"initialize","params":{}}"#,
		),
	);

	assert_eq!(response.status, "HTTP/1.1 202 Accepted");
	assert_eq!(response.header("mcp-session-id"), None);

	let response = run_http(
		&mut handler,
		http_post(
			"/mcp",
			[
				("Origin", "http://127.0.0.1:8193"),
				("Mcp-Session-Id", "decodex-mcp-session-0000000000000001"),
			],
			r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
		),
	);
	let body = response.json_body();

	assert_eq!(response.status, "HTTP/1.1 404 Not Found");
	assert_eq!(body["error"]["message"], "Unknown MCP session");
}

#[test]
fn streamable_http_invalid_initialize_does_not_create_session() {
	let repo = test_repo();
	let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);
	let response = run_http(
		&mut handler,
		http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193")],
			r#"{"jsonrpc":"1.0","id":1,"method":"initialize","params":{}}"#,
		),
	);
	let body = response.json_body();

	assert_eq!(response.status, "HTTP/1.1 200 OK");
	assert_eq!(response.header("mcp-session-id"), None);
	assert_eq!(body["error"]["message"], "Invalid Request");
}

#[test]
fn streamable_http_rejects_disallowed_origin() {
	let repo = test_repo();
	let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);
	let response = run_http(
		&mut handler,
		http_post(
			"/mcp",
			[("Origin", "https://example.invalid")],
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
		),
	);
	let body = response.json_body();

	assert_eq!(response.status, "HTTP/1.1 403 Forbidden");
	assert_eq!(body["error"]["message"], "Forbidden origin");
}

#[test]
fn streamable_http_delete_invalidates_session() {
	let repo = test_repo();
	let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);
	let initialize = run_http(
		&mut handler,
		http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193")],
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
		),
	);
	let session_id = initialize.header("mcp-session-id").expect("session id").to_owned();
	let delete = run_http(
		&mut handler,
		http_delete(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193"), ("Mcp-Session-Id", session_id.as_str())],
		),
	);

	assert_eq!(delete.status, "HTTP/1.1 202 Accepted");
	assert_eq!(delete.header("access-control-allow-origin"), Some("http://127.0.0.1:8193"));

	let response = run_http(
		&mut handler,
		http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193"), ("Mcp-Session-Id", session_id.as_str())],
			r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
		),
	);
	let body = response.json_body();

	assert_eq!(response.status, "HTTP/1.1 404 Not Found");
	assert_eq!(body["error"]["message"], "Unknown MCP session");
}

#[test]
fn streamable_http_bind_guard_requires_loopback_or_allowed_origin() {
	assert!(
		mcp::validate_mcp_http_listen_address(
			DEFAULT_MCP_HTTP_LISTEN_ADDRESS,
			&[],
			&McpHttpAuthorization::disabled()
		)
		.is_ok()
	);
	assert!(
		mcp::validate_mcp_http_listen_address(
			"0.0.0.0:8193",
			&[],
			&McpHttpAuthorization::disabled()
		)
		.is_err()
	);
	assert!(
		mcp::validate_mcp_http_listen_address(
			"0.0.0.0:8193",
			&[String::from("https://relay.example")],
			&McpHttpAuthorization::disabled()
		)
		.is_err()
	);
	assert!(
		mcp::validate_mcp_http_listen_address(
			"0.0.0.0:8193",
			&[String::from("https://relay.example")],
			&McpHttpAuthorization::from_token_for_test("secret-token")
		)
		.is_ok()
	);
}

#[test]
fn streamable_http_elevated_profile_requires_bearer_authorization() {
	assert!(
		mcp::validate_mcp_http_capability_profile(
			McpCapabilityProfile::Observe,
			&McpHttpAuthorization::disabled()
		)
		.is_ok()
	);

	for profile in
		[McpCapabilityProfile::Plan, McpCapabilityProfile::Operate, McpCapabilityProfile::Admin]
	{
		assert!(
			mcp::validate_mcp_http_capability_profile(profile, &McpHttpAuthorization::disabled())
				.is_err()
		);
		assert!(
			mcp::validate_mcp_http_capability_profile(
				profile,
				&McpHttpAuthorization::from_token_for_test("secret-token")
			)
			.is_ok()
		);
	}
}

#[test]
fn streamable_http_requires_known_session_after_initialize() {
	let repo = test_repo();
	let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);

	run_http(
		&mut handler,
		http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193")],
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
		),
	);

	let response = run_http(
		&mut handler,
		http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193")],
			r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
		),
	);
	let body = response.json_body();

	assert_eq!(response.status, "HTTP/1.1 428 Precondition Required");
	assert_eq!(body["error"]["message"], "Missing MCP session");
}

#[test]
fn streamable_http_observe_profile_exposes_only_observe_tool() {
	let repo = test_repo();
	let mut handler = http_handler(repo.path(), McpCapabilityProfile::Observe);
	let initialize = run_http(
		&mut handler,
		http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193")],
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
		),
	);
	let session_id = initialize.header("mcp-session-id").expect("session id").to_owned();
	let response = run_http(
		&mut handler,
		http_post(
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
	let repo = test_repo();
	let mut handler = http_handler(repo.path(), McpCapabilityProfile::Observe);
	let initialize = run_http(
		&mut handler,
		http_post(
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
		let response = run_http(
			&mut handler,
			http_post(
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
