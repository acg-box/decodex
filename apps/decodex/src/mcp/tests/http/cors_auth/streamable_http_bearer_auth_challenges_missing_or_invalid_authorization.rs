use crate::mcp::{McpCapabilityProfile, McpHttpAuthorization, tests::support};

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
