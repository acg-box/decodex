use serde_json::Value;

use super::support::{
	accepted_mcp_goal_contract, assert_tool_output_schema_variant,
	latent_decision_contract_fixture, response_at, run_stdio, run_stdio_with_context,
	run_stdio_with_profile, test_repo, write_decodex_project_config, write_decodex_workflow,
};
use crate::{
	mcp::{McpCapabilityProfile, McpContext},
	state::StateStore,
};

#[test]
fn prompts_list_and_get_return_prompt_messages() {
	let repo = test_repo();
	let list_responses =
		run_stdio(repo.path(), r#"{"jsonrpc":"2.0","id":1,"method":"prompts/list","params":{}}"#);
	let prompts =
		response_at(&list_responses, 0)["result"]["prompts"].as_array().expect("prompts array");
	let prompt_names = prompts
		.iter()
		.filter_map(|prompt| prompt.get("name").and_then(Value::as_str))
		.collect::<Vec<_>>();

	assert!(prompt_names.contains(&"decodex_validation_ready"));

	let get_responses = run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":2,"method":"prompts/get","params":{"name":"decodex_validation_ready","arguments":{"issue":"XY-994"}}}"#,
	);
	let messages =
		response_at(&get_responses, 0)["result"]["messages"].as_array().expect("messages array");
	let text = messages[0]["content"]["text"].as_str().expect("prompt text");

	assert!(text.contains("XY-994"));
	assert!(text.contains("validation-ready"));
}

#[test]
fn prompts_get_rejects_missing_required_arguments() {
	let repo = test_repo();
	let responses = run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"prompts/get","params":{"name":"decodex_validation_ready","arguments":{}}}"#,
	);
	let error = response_at(&responses, 0).get("error").expect("error response");

	assert_eq!(error["code"], -32_602);
}

#[test]
fn tools_list_exposes_schema_bound_tools() {
	let repo = test_repo();
	let responses =
		run_stdio(repo.path(), r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#);
	let tools = response_at(&responses, 0)["result"]["tools"].as_array().expect("tools array");
	let tool_names = tools
		.iter()
		.filter_map(|tool| tool.get("name").and_then(Value::as_str))
		.collect::<Vec<_>>();
	let plan = tools
		.iter()
		.find(|tool| tool.get("name").and_then(Value::as_str) == Some("decodex_plan"))
		.expect("plan tool should be listed");

	for tool_name in ["research_compile", "research_promote", "intake_goal"] {
		assert!(tool_names.contains(&tool_name), "{tool_name} should be listed");

		let tool = tools
			.iter()
			.find(|tool| tool.get("name").and_then(Value::as_str) == Some(tool_name))
			.expect("planning tool should exist");

		assert!(tool.get("inputSchema").is_some());
		assert!(tool.get("outputSchema").is_some());
		assert_eq!(tool["_meta"]["decodex/capabilityProfile"], "plan");
	}

	assert!(plan.get("inputSchema").is_some());
	assert!(plan.get("outputSchema").is_some());
	assert_eq!(plan["_meta"]["decodex/capabilityProfile"], "plan");

	assert_tool_output_schema_variant(plan, "decodex.mcp.plan_result/1", Some("next_action"));
	assert_tool_output_schema_variant(plan, "decodex.mcp.refusal/1", Some("reason"));
	assert_tool_output_schema_variant(plan, "decodex.mcp.tool_validation_error/1", Some("tool"));
	assert_tool_output_schema_variant(
		tools
			.iter()
			.find(|tool| tool.get("name").and_then(Value::as_str) == Some("research_compile"))
			.expect("research_compile tool should exist"),
		"decodex.mcp.research_compile_result/1",
		Some("contract_id"),
	);
	assert_tool_output_schema_variant(
		tools
			.iter()
			.find(|tool| tool.get("name").and_then(Value::as_str) == Some("research_promote"))
			.expect("research_promote tool should exist"),
		"decodex.mcp.research_promote_result/1",
		Some("contract_id"),
	);
	assert_tool_output_schema_variant(
		tools
			.iter()
			.find(|tool| tool.get("name").and_then(Value::as_str) == Some("intake_goal"))
			.expect("intake_goal tool should exist"),
		"decodex.mcp.intake_goal_result/1",
		Some("issues"),
	);
}

#[test]
fn tools_list_filters_by_active_capability_profile() {
	let repo = test_repo();
	let responses = run_stdio_with_profile(
		repo.path(),
		McpCapabilityProfile::Observe,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#,
	);
	let tools = response_at(&responses, 0)["result"]["tools"].as_array().expect("tools array");
	let tool_names = tools
		.iter()
		.filter_map(|tool| tool.get("name").and_then(Value::as_str))
		.collect::<Vec<_>>();

	assert_eq!(tool_names, vec!["decodex_observe"]);
}

#[test]
fn tools_call_refuses_tools_above_active_capability_profile() {
	let repo = test_repo();
	let responses = run_stdio_with_profile(
		repo.path(),
		McpCapabilityProfile::Observe,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}"#,
	);
	let structured = &response_at(&responses, 0)["result"]["structuredContent"];

	assert_eq!(structured["schema"], "decodex.mcp.refusal/1");
	assert_eq!(structured["reason"], "insufficient_capability_profile");
	assert_eq!(structured["capability_profile"], "observe");
	assert_eq!(structured["required_capability_profile"], "plan");
}

#[test]
fn tools_call_returns_structured_content() {
	let repo = test_repo();
	let responses = run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_plan","arguments":{"intent":"validation_ready","issue":"XY-994"}}}"#,
	);
	let structured = &response_at(&responses, 0)["result"]["structuredContent"];

	assert_eq!(structured["schema"], "decodex.mcp.plan_result/1");
	assert_eq!(structured["status"], "ok");
	assert_eq!(structured["issue"], "XY-994");
}

#[test]
fn tools_call_research_compile_dry_run_returns_structured_contract() {
	let repo = test_repo();
	let context = McpContext {
		repo_root: repo.path().to_path_buf(),
		config_path: None,
		project_id: Some(String::from("decodex")),
		state_store: None,
	};
	let responses = run_stdio_with_context(
		context,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"research_compile","arguments":{"mode":"dry_run","intent":"research schema-bound MCP planning","outcome":"not_decision_ready"}}}"#,
	);
	let result = &response_at(&responses, 0)["result"];
	let structured = &result["structuredContent"];

	assert_eq!(result["isError"], false);
	assert_eq!(structured["schema"], "decodex.mcp.research_compile_result/1");
	assert_eq!(structured["status"], "ok");
	assert_eq!(structured["mode"], "dry_run");
	assert_eq!(structured["persisted"], false);
	assert_eq!(structured["contract_status"], "draft_latent");
	assert_eq!(structured["execution_authority_granted"], false);
}

#[test]
fn tools_call_research_compile_apply_requires_authority() {
	let repo = test_repo();
	let context = McpContext {
		repo_root: repo.path().to_path_buf(),
		config_path: None,
		project_id: Some(String::from("decodex")),
		state_store: None,
	};
	let responses = run_stdio_with_context(
		context,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"research_compile","arguments":{"mode":"apply","intent":"research schema-bound MCP planning","outcome":"not_decision_ready"}}}"#,
	);
	let result = &response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
	assert_eq!(result["structuredContent"]["reason"], "missing_authority");
	assert_eq!(result["structuredContent"]["tool"], "research_compile");
}

#[test]
fn tools_call_research_promote_defaults_to_dry_run() {
	let repo = test_repo();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_decision_contract("decodex", Some("XY-852"), latent_decision_contract_fixture())
		.expect("decision contract should persist");

	let context = McpContext {
		repo_root: repo.path().to_path_buf(),
		config_path: None,
		project_id: Some(String::from("decodex")),
		state_store: Some(state_store),
	};
	let responses = run_stdio_with_context(
		context,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"research_promote","arguments":{"contractId":"research-x-loop-contract"}}}"#,
	);
	let result = &response_at(&responses, 0)["result"];
	let structured = &result["structuredContent"];

	assert_eq!(result["isError"], false);
	assert_eq!(structured["schema"], "decodex.mcp.research_promote_result/1");
	assert_eq!(structured["mode"], "dry_run");
	assert_eq!(structured["persisted"], false);
	assert_eq!(structured["contract_id"], "research-x-loop-contract");
}

#[test]
fn tools_call_research_promote_apply_requires_authority() {
	let repo = test_repo();
	let context = McpContext {
		repo_root: repo.path().to_path_buf(),
		config_path: None,
		project_id: Some(String::from("decodex")),
		state_store: Some(StateStore::open_in_memory().expect("state store should open")),
	};
	let responses = run_stdio_with_context(
		context,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"research_promote","arguments":{"mode":"apply","contractId":"research-design-contract"}}}"#,
	);
	let result = &response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
	assert_eq!(result["structuredContent"]["reason"], "missing_authority");
	assert_eq!(result["structuredContent"]["tool"], "research_promote");
}

#[test]
fn tools_call_intake_goal_dry_run_does_not_persist_program_intake() {
	let repo = test_repo();
	let db_path = repo.path().join("runtime.sqlite3");
	let seed_store = StateStore::open(&db_path).expect("state store should open");

	seed_store
		.upsert_decision_contract("decodex", Some("XY-852"), accepted_mcp_goal_contract())
		.expect("contract should persist");

	let config_path = repo.path().join("project.toml");

	write_decodex_project_config(&config_path, repo.path());
	write_decodex_workflow(repo.path());

	let context = McpContext {
		repo_root: repo.path().to_path_buf(),
		config_path: Some(config_path),
		project_id: Some(String::from("decodex")),
		state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
	};
	let responses = run_stdio_with_context(
		context,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"intake_goal","arguments":{"mode":"dry_run","contractId":"mcp-goal-contract"}}}"#,
	);
	let result = &response_at(&responses, 0)["result"];
	let structured = &result["structuredContent"];

	assert_eq!(result["isError"], false);
	assert_eq!(structured["schema"], "decodex.mcp.intake_goal_result/1");
	assert_eq!(structured["mode"], "dry_run");
	assert_eq!(structured["persisted"], false);
	assert_eq!(structured["issue_count"], 1);
	assert_eq!(structured["issues"][0]["action"], "would_create");
	assert!(structured["issues"][0].get("node_id").is_none());
	assert!(structured.get("program_id").is_none());

	let readback = StateStore::open(&db_path).expect("state store should reopen");

	assert!(
		readback
			.list_program_intake_plans("decodex")
			.expect("program intake plans should list")
			.is_empty()
	);
}

#[test]
fn tools_call_intake_goal_apply_requires_authority() {
	let repo = test_repo();
	let responses = run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"intake_goal","arguments":{"mode":"apply","contractId":"mcp-goal-contract"}}}"#,
	);
	let result = &response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
	assert_eq!(result["structuredContent"]["reason"], "missing_authority");
	assert_eq!(result["structuredContent"]["tool"], "intake_goal");
}
