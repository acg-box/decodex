use crate::mcp::tests::support::{self};

#[test]
fn tools_call_refuses_lane_control_mutation_without_inspect_precondition() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"interrupt","issue":"XY-994","runId":"run-1"}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.lane_control_result/1");
	assert_eq!(result["structuredContent"]["status"], "refused");
	assert_eq!(result["structuredContent"]["reason"], "authority_required");
}

#[test]
fn tools_call_refuses_missing_lane_control_action() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"issue":"XY-994"}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.tool_validation_error/1");
	assert_eq!(result["structuredContent"]["reason"], "invalid_arguments");
	assert_eq!(result["structuredContent"]["tool"], "decodex_lane_control");
}

#[test]
fn tools_call_lane_control_refuses_stale_expected_turn_id() {
	let repo = support::test_repo();
	let _runtime_home_guard = support::isolated_mcp_runtime_home(&repo);
	let config_path = repo.path().join("project.toml");

	support::seed_project_runtime_for_mcp_resources(repo.path(), &config_path);

	let responses = support::run_stdio_with_context(
		support::project_mcp_context(repo.path(), &config_path),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"steer","projectId":"pubfi","issue":"PUB-012","runId":"run-12","expectedTurnId":"turn-old","message":"Please stop after the current safe point.","authority":{"reason":"operator requested steer","source":"mcp-test","inspectedRunId":"run-12","expectedTurnId":"turn-old"}}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];
	let structured = &result["structuredContent"];

	assert_eq!(result["isError"], true);
	assert_eq!(structured["status"], "refused");
	assert_eq!(structured["reason"], "stale_expected_turn_id");
	assert_eq!(structured["result"]["failureClass"], "stale_expected_turn_id");
	assert_eq!(structured["result"]["currentTurnId"], "turn-12");

	support::assert_no_sensitive_observability_content(structured);
}
