use crate::mcp::{McpCapabilityProfile, tests::support};

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
