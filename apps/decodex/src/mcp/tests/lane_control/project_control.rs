use crate::{
	mcp::{
		McpContext,
		tests::support::{self},
	},
	runtime,
};

#[test]
fn tools_call_project_control_pauses_future_dispatch_only() {
	let repo = support::test_repo();
	let _runtime_home_guard = support::isolated_mcp_runtime_home(&repo);
	let config_path = repo.path().join("project.toml");

	support::seed_project_runtime_for_mcp_resources(repo.path(), &config_path);
	support::seed_mcp_test_private_control_evidence();

	let responses = support::run_stdio_with_context(
		support::project_mcp_context(repo.path(), &config_path),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_project_control","arguments":{"action":"pause","projectId":"pubfi","authority":{"reason":"operator pause","source":"mcp-test","acknowledgeFutureDispatchOnly":true}}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];
	let structured = &result["structuredContent"];

	assert_eq!(result["isError"], false);
	assert_eq!(structured["schema"], "decodex.mcp.project_control_result/1");
	assert_eq!(structured["status"], "ok");
	assert_eq!(structured["project_id"], "pubfi");
	assert_eq!(structured["future_dispatch_only"], true);
	assert_eq!(structured["result"]["enabled"], false);
	assert_eq!(structured["result"]["active_lanes_killed"], false);

	let state_store = runtime::open_runtime_store().expect("runtime store should open");
	let projects = state_store.list_projects().expect("projects should list");
	let project = projects
		.iter()
		.find(|project| project.service_id() == "pubfi")
		.expect("pubfi should remain registered");
	let events = state_store
		.list_private_execution_events("pubfi", "PUB-012", "run-12", 1)
		.expect("private events should read");

	assert!(!project.enabled());
	assert!(!events.is_empty(), "pause should not remove active lane evidence");
}

#[test]
fn tools_call_project_control_scan_refuses_without_operator_loop() {
	let repo = support::test_repo();
	let responses = support::run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("pubfi")),
			state_store: None,
		},
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_project_control","arguments":{"action":"scan","projectId":"pubfi"}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.project_control_result/1");
	assert_eq!(result["structuredContent"]["reason"], "operator_control_loop_required");
}

#[test]
fn tools_call_refuses_missing_project_control_action() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_project_control","arguments":{}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.tool_validation_error/1");
	assert_eq!(result["structuredContent"]["reason"], "invalid_arguments");
	assert_eq!(result["structuredContent"]["tool"], "decodex_project_control");
}
