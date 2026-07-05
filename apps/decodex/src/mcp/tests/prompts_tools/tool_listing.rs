use serde_json::Value;

use crate::mcp::{
	McpCapabilityProfile,
	tests::support::{self},
};

#[test]
fn tools_list_exposes_schema_bound_tools() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#,
	);
	let tools =
		support::response_at(&responses, 0)["result"]["tools"].as_array().expect("tools array");
	let tool_names = tools
		.iter()
		.filter_map(|tool| tool.get("name").and_then(Value::as_str))
		.collect::<Vec<_>>();
	let plan = tools
		.iter()
		.find(|tool| tool.get("name").and_then(Value::as_str) == Some("decodex_plan"))
		.expect("plan tool should be listed");
	let autonomy_compile = tools
		.iter()
		.find(|tool| tool.get("name").and_then(Value::as_str) == Some("autonomy_compile_proposal"))
		.expect("autonomy_compile_proposal tool should exist");

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
	assert_eq!(
		autonomy_compile["inputSchema"]["properties"]["proposal"]["properties"]["issueCandidates"]
			["items"]["properties"]["dependencies"]["description"],
		"Candidate keys that must complete before this candidate."
	);

	support::assert_tool_output_schema_variant(
		plan,
		"decodex.mcp.plan_result/1",
		Some("next_action"),
	);
	support::assert_tool_output_schema_variant(plan, "decodex.mcp.refusal/1", Some("reason"));
	support::assert_tool_output_schema_variant(
		plan,
		"decodex.mcp.tool_validation_error/1",
		Some("tool"),
	);
	support::assert_tool_output_schema_variant(
		tools
			.iter()
			.find(|tool| tool.get("name").and_then(Value::as_str) == Some("research_compile"))
			.expect("research_compile tool should exist"),
		"decodex.mcp.research_compile_result/1",
		Some("contract_id"),
	);
	support::assert_tool_output_schema_variant(
		tools
			.iter()
			.find(|tool| tool.get("name").and_then(Value::as_str) == Some("research_promote"))
			.expect("research_promote tool should exist"),
		"decodex.mcp.research_promote_result/1",
		Some("contract_id"),
	);
	support::assert_tool_output_schema_variant(
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
	let repo = support::test_repo();
	let responses = support::run_stdio_with_profile(
		repo.path(),
		McpCapabilityProfile::Observe,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#,
	);
	let tools =
		support::response_at(&responses, 0)["result"]["tools"].as_array().expect("tools array");
	let tool_names = tools
		.iter()
		.filter_map(|tool| tool.get("name").and_then(Value::as_str))
		.collect::<Vec<_>>();

	assert_eq!(tool_names, vec!["decodex_observe"]);
}
