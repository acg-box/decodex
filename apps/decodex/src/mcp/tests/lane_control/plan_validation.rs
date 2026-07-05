use crate::mcp::tests::support::{self};

#[test]
fn tools_call_refuses_missing_plan_intent() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_plan","arguments":{}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.tool_validation_error/1");
	assert_eq!(result["structuredContent"]["reason"], "invalid_arguments");
	assert_eq!(result["structuredContent"]["tool"], "decodex_plan");
}
