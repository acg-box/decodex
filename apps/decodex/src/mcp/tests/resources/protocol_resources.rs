use serde_json::Value;

use crate::{
	mcp::{
		self, McpContext, ResourceContent,
		tests::support::{self},
	},
	state::StateStore,
};

#[test]
fn initialize_exposes_protocol_primitive_capabilities() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
	);
	let response = support::response_at(&responses, 0);
	let result = response.get("result").and_then(Value::as_object).expect("result object");
	let capabilities =
		result.get("capabilities").and_then(Value::as_object).expect("capabilities object");

	assert!(capabilities.contains_key("resources"));
	assert!(capabilities.contains_key("prompts"));
	assert!(capabilities.contains_key("tools"));
	assert!(capabilities.contains_key("logging"));
	assert_eq!(capabilities["experimental"]["decodex"]["capabilityProfile"], "admin");
}

#[test]
fn logging_set_level_is_stdio_compatible() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"logging/setLevel","params":{"level":"debug"}}"#,
	);
	let result = support::response_at(&responses, 0)["result"].as_object().expect("result object");

	assert!(result.is_empty());
}

#[test]
fn resources_list_includes_openwiki_pages() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"resources/list","params":{}}"#,
	);
	let resources = support::response_at(&responses, 0)["result"]["resources"]
		.as_array()
		.expect("resources array");
	let uris = resources
		.iter()
		.filter_map(|resource| resource.get("uri").and_then(Value::as_str))
		.collect::<Vec<_>>();

	assert!(uris.contains(&"decodex://openwiki/quickstart"));
	assert!(uris.contains(&"decodex://openwiki/specs/contracts-and-data"));
	assert!(uris.contains(&"decodex://openwiki/workflows/runtime-operator-workflows"));
}

#[test]
fn resources_list_includes_runtime_decision_contracts() {
	let repo = support::test_repo();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_decision_contract(
			"decodex",
			Some("XY-852"),
			support::latent_decision_contract_fixture(),
		)
		.expect("decision contract should persist");

	let responses = support::run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(state_store),
		},
		r#"{"jsonrpc":"2.0","id":1,"method":"resources/list","params":{}}"#,
	);
	let resources = support::response_at(&responses, 0)["result"]["resources"]
		.as_array()
		.expect("resources array");
	let uris = resources
		.iter()
		.filter_map(|resource| resource.get("uri").and_then(Value::as_str))
		.collect::<Vec<_>>();

	assert!(uris.contains(&"decodex://decision-contracts/decision-x-loop-contract"));
}

#[test]
fn resources_read_returns_runtime_decision_contract() {
	let repo = support::test_repo();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_decision_contract(
			"decodex",
			Some("XY-852"),
			support::latent_decision_contract_fixture(),
		)
		.expect("decision contract should persist");

	let responses = support::run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(state_store),
		},
		r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://decision-contracts/decision-x-loop-contract"}}"#,
	);
	let contents = support::response_at(&responses, 0)["result"]["contents"]
		.as_array()
		.expect("contents array");
	let text = contents[0]["text"].as_str().expect("text content");
	let content: Value = serde_json::from_str(text).expect("decision contract should be json");

	assert_eq!(content["project_id"], "decodex");
	assert_eq!(content["decision_contract"]["contract_id"], "decision-x-loop-contract");
	assert!(content["decision_contract"]["evidence_boundary"]["private_evidence_refs"].is_null());
	assert!(content["decision_contract"]["links"]["execution_program_node_ids"].is_null());
	assert!(!text.contains("decision-x-run"));
}

#[test]
fn resources_read_returns_checked_in_openwiki_text() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://openwiki/specs/contracts-and-data"}}"#,
	);
	let contents = support::response_at(&responses, 0)["result"]["contents"]
		.as_array()
		.expect("contents array");
	let text = contents[0]["text"].as_str().expect("text content");

	assert_eq!(text, "# Contracts\n\nSpec body.\n");
}

#[test]
fn observability_sanitizer_strips_private_operator_fields() {
	let mut value = support::sensitive_observability_fixture();

	mcp::sanitize_mcp_observability_value(&mut value);
	support::assert_observability_is_sanitized(&value);
}

#[test]
fn observability_resource_content_strips_private_operator_fields() {
	let content = ResourceContent::mcp_observability_json(
		"decodex://projects/decodex/status",
		support::sensitive_observability_fixture(),
	)
	.expect("observability content should serialize");
	let value: Value = serde_json::from_str(&content.text).expect("content should be json");

	assert_eq!(content.mime_type, "application/json");

	support::assert_observability_is_sanitized(&value);
}
