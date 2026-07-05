use crate::mcp::{
	McpCapabilityProfile,
	tests::support::{self},
};

#[test]
fn tools_call_refuses_tools_above_active_capability_profile() {
	let repo = support::test_repo();
	let responses = support::run_stdio_with_profile(
		repo.path(),
		McpCapabilityProfile::Observe,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}"#,
	);
	let structured = &support::response_at(&responses, 0)["result"]["structuredContent"];

	assert_eq!(structured["schema"], "decodex.mcp.refusal/1");
	assert_eq!(structured["reason"], "insufficient_capability_profile");
	assert_eq!(structured["capability_profile"], "observe");
	assert_eq!(structured["required_capability_profile"], "plan");
}

#[test]
fn tools_call_returns_structured_content() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_plan","arguments":{"intent":"validation_ready","issue":"XY-994"}}}"#,
	);
	let structured = &support::response_at(&responses, 0)["result"]["structuredContent"];

	assert_eq!(structured["schema"], "decodex.mcp.plan_result/1");
	assert_eq!(structured["status"], "ok");
	assert_eq!(structured["issue"], "XY-994");
}
