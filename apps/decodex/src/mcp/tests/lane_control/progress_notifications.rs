use crate::mcp::{
	McpCapabilityProfile,
	tests::support::{self},
};

#[test]
fn tools_call_returns_structured_refusal_for_invalid_observe_arguments() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_observe","arguments":{"limit":0}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["status"], "refused");
	assert_eq!(result["structuredContent"]["reason"], "invalid_limit");
}

#[test]
fn tools_call_emits_json_rpc_progress_notification_when_requested() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"},"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}"#,
	);

	assert_eq!(responses[0]["method"], "notifications/progress");
	assert_eq!(responses[0]["params"]["progressToken"], "progress-1");
	assert_eq!(responses[1]["result"]["structuredContent"]["status"], "ok");
}

#[test]
fn tools_call_does_not_emit_progress_for_invalid_params() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"}}}"#,
	);

	assert_eq!(responses.len(), 1);
	assert_eq!(responses[0]["id"], 1);
	assert_eq!(responses[0]["error"]["code"], -32_602);
}

#[test]
fn tools_call_does_not_emit_progress_for_structured_validation_error() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"},"name":"decodex_plan","arguments":{}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(responses.len(), 1);
	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.tool_validation_error/1");
}

#[test]
fn tools_call_does_not_emit_progress_for_structured_refusal() {
	let repo = support::test_repo();
	let responses = support::run_stdio_with_profile(
		repo.path(),
		McpCapabilityProfile::Observe,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"},"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(responses.len(), 1);
	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
	assert_eq!(result["structuredContent"]["reason"], "insufficient_capability_profile");
}
