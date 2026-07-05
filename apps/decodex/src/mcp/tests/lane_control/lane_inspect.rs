use crate::mcp::tests::support::{self};

#[test]
fn tools_call_lane_control_inspect_returns_mutating_preconditions() {
	let repo = support::test_repo();
	let _runtime_home_guard = support::isolated_mcp_runtime_home(&repo);
	let config_path = repo.path().join("project.toml");

	support::seed_project_runtime_for_mcp_resources(repo.path(), &config_path);

	let responses = support::run_stdio_with_context(
		support::project_mcp_context(repo.path(), &config_path),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"inspect","issue":"PUB-012"}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];
	let structured = &result["structuredContent"];

	assert_eq!(result["isError"], false);
	assert_eq!(structured["schema"], "decodex.mcp.lane_control_result/1");
	assert_eq!(structured["status"], "ok");
	assert_eq!(structured["reason"], "inspect_complete");
	assert_eq!(structured["result"]["inspect"]["schema"], "decodex.mcp.lane_inspect/1");
	assert_eq!(
		structured["result"]["mutating_preconditions"][0]["authority"]["inspectedRunId"],
		"run-12"
	);
	assert_eq!(
		structured["result"]["mutating_preconditions"][0]["authority"]["expectedTurnId"],
		"turn-12"
	);

	support::assert_no_sensitive_observability_content(structured);
}
