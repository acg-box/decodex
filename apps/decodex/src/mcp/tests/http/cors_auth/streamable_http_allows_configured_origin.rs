use crate::mcp::{McpCapabilityProfile, tests::support};

#[test]
fn streamable_http_allows_configured_origin() {
	let repo = support::test_repo();
	let mut handler = support::http_handler_with_allowed_origins(
		repo.path(),
		McpCapabilityProfile::Admin,
		vec![String::from("https://relay.example")],
	);
	let preflight = support::run_http(
		&mut handler,
		support::http_options(
			"/mcp",
			[("Origin", "https://relay.example"), ("Access-Control-Request-Method", "POST")],
		),
	);
	let initialize = support::run_http(
		&mut handler,
		support::http_post(
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
