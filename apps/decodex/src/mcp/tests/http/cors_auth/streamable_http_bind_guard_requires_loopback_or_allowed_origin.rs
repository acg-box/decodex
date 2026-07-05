use crate::mcp::{DEFAULT_MCP_HTTP_LISTEN_ADDRESS, McpHttpAuthorization, http};

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
