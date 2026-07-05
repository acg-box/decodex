use crate::mcp::{McpCapabilityProfile, McpHttpAuthorization, tests::support};

#[test]
fn streamable_http_bearer_auth_accepts_valid_authorization() {
	let repo = support::test_repo();
	let mut handler = support::http_handler_with_authorization(
		repo.path(),
		McpCapabilityProfile::Observe,
		McpHttpAuthorization::from_token_for_test("secret-token"),
	);
	let preflight = support::run_http(
		&mut handler,
		support::http_options(
			"/mcp",
			[
				("Origin", "http://127.0.0.1:8193"),
				("Access-Control-Request-Method", "POST"),
				("Access-Control-Request-Headers", "Authorization, Content-Type"),
			],
		),
	);
	let response = support::run_http(
		&mut handler,
		support::http_post(
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
