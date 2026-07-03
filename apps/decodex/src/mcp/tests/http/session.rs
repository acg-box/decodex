use crate::mcp::{McpCapabilityProfile, tests::support};

#[test]
fn streamable_http_json_post_initializes_session() {
	let repo = support::test_repo();
	let mut handler = support::http_handler(repo.path(), McpCapabilityProfile::Admin);
	let response = support::run_http(
		&mut handler,
		support::http_post(
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
fn streamable_http_initialize_notification_does_not_create_session() {
	let repo = support::test_repo();
	let mut handler = support::http_handler(repo.path(), McpCapabilityProfile::Admin);
	let response = support::run_http(
		&mut handler,
		support::http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193")],
			r#"{"jsonrpc":"2.0","method":"initialize","params":{}}"#,
		),
	);

	assert_eq!(response.status, "HTTP/1.1 202 Accepted");
	assert_eq!(response.header("mcp-session-id"), None);

	let response = support::run_http(
		&mut handler,
		support::http_post(
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
	let repo = support::test_repo();
	let mut handler = support::http_handler(repo.path(), McpCapabilityProfile::Admin);
	let response = support::run_http(
		&mut handler,
		support::http_post(
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
fn streamable_http_delete_invalidates_session() {
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
	let delete = support::run_http(
		&mut handler,
		support::http_delete(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193"), ("Mcp-Session-Id", session_id.as_str())],
		),
	);

	assert_eq!(delete.status, "HTTP/1.1 202 Accepted");
	assert_eq!(delete.header("access-control-allow-origin"), Some("http://127.0.0.1:8193"));

	let response = support::run_http(
		&mut handler,
		support::http_post(
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
fn streamable_http_requires_known_session_after_initialize() {
	let repo = support::test_repo();
	let mut handler = support::http_handler(repo.path(), McpCapabilityProfile::Admin);

	support::run_http(
		&mut handler,
		support::http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193")],
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
		),
	);

	let response = support::run_http(
		&mut handler,
		support::http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193")],
			r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
		),
	);
	let body = response.json_body();

	assert_eq!(response.status, "HTTP/1.1 428 Precondition Required");
	assert_eq!(body["error"]["message"], "Missing MCP session");
}
