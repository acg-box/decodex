use crate::{
	mcp::{
		McpContext,
		tests::support::{self},
	},
	state::StateStore,
};

#[test]
fn tools_call_intake_goal_dry_run_does_not_persist_program_intake() {
	let repo = support::test_repo();
	let db_path = repo.path().join("runtime.sqlite3");
	let seed_store = StateStore::open(&db_path).expect("state store should open");

	seed_store
		.upsert_decision_contract("decodex", Some("XY-852"), support::accepted_mcp_goal_contract())
		.expect("contract should persist");

	let config_path = repo.path().join("project.toml");

	support::write_decodex_project_config(&config_path, repo.path());
	support::write_decodex_workflow(repo.path());

	let context = McpContext {
		config_path: Some(config_path),
		project_id: Some(String::from("decodex")),
		state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
	};
	let responses = support::run_stdio_with_context(
		context,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"intake_goal","arguments":{"mode":"dry_run","contractId":"mcp-goal-contract"}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];
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
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"intake_goal","arguments":{"mode":"apply","contractId":"mcp-goal-contract"}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
	assert_eq!(result["structuredContent"]["reason"], "missing_authority");
	assert_eq!(result["structuredContent"]["tool"], "intake_goal");
}
