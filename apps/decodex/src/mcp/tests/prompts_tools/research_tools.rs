use crate::{
	mcp::{
		McpContext,
		tests::support::{self},
	},
	state::StateStore,
};

#[test]
fn tools_call_research_compile_dry_run_returns_structured_contract() {
	let repo = support::test_repo();
	let context = McpContext {
		repo_root: repo.path().to_path_buf(),
		config_path: None,
		project_id: Some(String::from("decodex")),
		state_store: None,
	};
	let responses = support::run_stdio_with_context(
		context,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"research_compile","arguments":{"mode":"dry_run","intent":"research schema-bound MCP planning","outcome":"not_decision_ready"}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];
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
	let repo = support::test_repo();
	let context = McpContext {
		repo_root: repo.path().to_path_buf(),
		config_path: None,
		project_id: Some(String::from("decodex")),
		state_store: None,
	};
	let responses = support::run_stdio_with_context(
		context,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"research_compile","arguments":{"mode":"apply","intent":"research schema-bound MCP planning","outcome":"not_decision_ready"}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
	assert_eq!(result["structuredContent"]["reason"], "missing_authority");
	assert_eq!(result["structuredContent"]["tool"], "research_compile");
}

#[test]
fn tools_call_research_promote_defaults_to_dry_run() {
	let repo = support::test_repo();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_decision_contract(
			"decodex",
			Some("XY-852"),
			support::latent_decision_contract_fixture(),
		)
		.expect("decision contract should persist");

	let context = McpContext {
		repo_root: repo.path().to_path_buf(),
		config_path: None,
		project_id: Some(String::from("decodex")),
		state_store: Some(state_store),
	};
	let responses = support::run_stdio_with_context(
		context,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"research_promote","arguments":{"contractId":"research-x-loop-contract"}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];
	let structured = &result["structuredContent"];

	assert_eq!(result["isError"], false);
	assert_eq!(structured["schema"], "decodex.mcp.research_promote_result/1");
	assert_eq!(structured["mode"], "dry_run");
	assert_eq!(structured["persisted"], false);
	assert_eq!(structured["contract_id"], "research-x-loop-contract");
}

#[test]
fn tools_call_research_promote_apply_requires_authority() {
	let repo = support::test_repo();
	let context = McpContext {
		repo_root: repo.path().to_path_buf(),
		config_path: None,
		project_id: Some(String::from("decodex")),
		state_store: Some(StateStore::open_in_memory().expect("state store should open")),
	};
	let responses = support::run_stdio_with_context(
		context,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"research_promote","arguments":{"mode":"apply","contractId":"research-design-contract"}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
	assert_eq!(result["structuredContent"]["reason"], "missing_authority");
	assert_eq!(result["structuredContent"]["tool"], "research_promote");
}
