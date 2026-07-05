use crate::mcp::{McpCapabilityProfile, tests::support};

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
