use crate::mcp::{
	DEFAULT_MCP_HTTP_LISTEN_ADDRESS, McpCapabilityProfile, McpHttpAuthorization, http,
	tests::support,
};

#[test]
fn streamable_http_allows_cors_preflight_for_trusted_origin() {
	let repo = support::test_repo();
	let mut handler = support::http_handler(repo.path(), McpCapabilityProfile::Admin);
	let response = support::run_http(
		&mut handler,
		support::http_options(
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
	let repo = support::test_repo();
	let mut handler = support::http_handler_with_authorization(
		repo.path(),
		McpCapabilityProfile::Observe,
		McpHttpAuthorization::from_token_for_test("secret-token"),
	);

	for headers in [
		vec![("Origin", "http://127.0.0.1:8193")],
		vec![("Origin", "http://127.0.0.1:8193"), ("Authorization", "Bearer wrong-token")],
	] {
		let response = support::run_http(
			&mut handler,
			support::http_post(
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

#[test]
fn streamable_http_rejects_disallowed_origin() {
	let repo = support::test_repo();
	let mut handler = support::http_handler(repo.path(), McpCapabilityProfile::Admin);
	let response = support::run_http(
		&mut handler,
		support::http_post(
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
fn streamable_http_bind_guard_requires_loopback_or_allowed_origin() {
	assert!(
		http::validate_mcp_http_listen_address(
			DEFAULT_MCP_HTTP_LISTEN_ADDRESS,
			&[],
			&McpHttpAuthorization::disabled()
		)
		.is_ok()
	);
	assert!(
		http::validate_mcp_http_listen_address(
			"0.0.0.0:8193",
			&[],
			&McpHttpAuthorization::disabled()
		)
		.is_err()
	);
	assert!(
		http::validate_mcp_http_listen_address(
			"0.0.0.0:8193",
			&[String::from("https://relay.example")],
			&McpHttpAuthorization::disabled()
		)
		.is_err()
	);
	assert!(
		http::validate_mcp_http_listen_address(
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
		http::validate_mcp_http_capability_profile(
			McpCapabilityProfile::Observe,
			&McpHttpAuthorization::disabled()
		)
		.is_ok()
	);

	for profile in
		[McpCapabilityProfile::Plan, McpCapabilityProfile::Operate, McpCapabilityProfile::Admin]
	{
		assert!(
			http::validate_mcp_http_capability_profile(profile, &McpHttpAuthorization::disabled())
				.is_err()
		);
		assert!(
			http::validate_mcp_http_capability_profile(
				profile,
				&McpHttpAuthorization::from_token_for_test("secret-token")
			)
			.is_ok()
		);
	}
}
