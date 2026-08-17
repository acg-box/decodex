use serde_json::Value;

use crate::{
	mcp::{
		McpCapabilityProfile, McpContext,
		tests::support::{self},
	},
	state::StateStore,
};

#[test]
fn autonomy_tools_are_plan_profile_and_apply_requires_authority() {
	let repo = support::test_repo();
	let observe_responses = support::run_stdio_with_profile(
		repo.path(),
		McpCapabilityProfile::Observe,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"autonomy_submit_signal","arguments":{"kind":"runtime_health","signal":{}}}}"#,
	);
	let observe_structured =
		&support::response_at(&observe_responses, 0)["result"]["structuredContent"];

	assert_eq!(observe_structured["reason"], "insufficient_capability_profile");
	assert_eq!(observe_structured["required_capability_profile"], "plan");

	let observe_accept_responses = support::run_stdio_with_profile(
		repo.path(),
		McpCapabilityProfile::Observe,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"autonomy_accept_objective","arguments":{"objectiveId":"quality-autonomy","objectiveVersion":1}}}"#,
	);
	let observe_accept_structured =
		&support::response_at(&observe_accept_responses, 0)["result"]["structuredContent"];

	assert_eq!(observe_accept_structured["reason"], "insufficient_capability_profile");
	assert_eq!(observe_accept_structured["required_capability_profile"], "plan");

	let state_store = StateStore::open_in_memory().expect("state store should open");
	let context = McpContext {
		config_path: None,
		project_id: Some(String::from("decodex")),
		state_store: Some(state_store),
	};
	let responses = support::run_stdio_with_context(
		context,
		r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"autonomy_draft_objective","arguments":{"mode":"apply","objective":{"schema":"decodex.autonomy_objective/1","record_version":1,"project_id":"decodex","id":"quality-autonomy","version":1,"state":"draft","summary":"Improve quality.","goals":["Reduce churn."],"non_goals":["Do not bypass authority."],"metrics":["Validation retry count."],"allowed_surfaces":["apps/decodex/src"],"allowed_signal_kinds":["runtime_health"],"validation_gates":["cargo make check"],"review_policy":"review required","memory_policy":"source-linked only","report_policy":"public-safe only"}}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
	assert_eq!(result["structuredContent"]["reason"], "missing_authority");
	assert_eq!(result["structuredContent"]["tool"], "autonomy_draft_objective");
}

#[test]
fn autonomy_accept_objective_accepts_draft_without_execution_authority() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let draft_call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"autonomy_draft_objective","arguments":{"mode":"apply","objective":{"schema":"decodex.autonomy_objective/1","record_version":1,"project_id":"decodex","id":"self-iteration-pilot","version":1,"state":"draft","summary":"Pilot Decodex self-iteration only on the decodex project.","goals":["Reduce repeated operator intervention.","Convert Decodex-only feedback into evidence-backed proposals."],"non_goals":["Do not touch other projects.","Do not bypass review, landing, install, or restart gates."],"metrics":["Manual-attention count.","Validated proposal replay completeness."],"allowed_surfaces":["apps/decodex/src","automations/decodex"],"allowed_signal_kinds":["runtime_health","protocol_drift","execution_friction","validation_regression","user_feedback_cluster"],"validation_gates":["cargo test -p decodex mcp --lib"],"review_policy":"challenge required before promotion","memory_policy":"source-linked evidence only","report_policy":"public-safe source refs with known gaps"},"authority":{"source":"mcp-test","reason":"store draft objective"}}}}"#;
	let accept_missing_authority_call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"autonomy_accept_objective","arguments":{"mode":"apply","objectiveId":"self-iteration-pilot","objectiveVersion":1}}}"#;
	let accept_call = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"autonomy_accept_objective","arguments":{"mode":"apply","objectiveId":"self-iteration-pilot","objectiveVersion":1,"authority":{"acceptedBy":"operator","acceptedByKind":"user","acceptedAt":"2026-06-27T00:00:00Z","acceptanceSource":"conversation"}}}}"#;
	let read_call = r#"{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"decodex://projects/decodex/autonomy/objectives/self-iteration-pilot/current"}}"#;
	let responses = support::run_stdio_with_context(
		McpContext {
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(state_store),
		},
		&format!("{draft_call}\n{accept_missing_authority_call}\n{accept_call}\n{read_call}"),
	);
	let draft_result = &support::response_at(&responses, 0)["result"]["structuredContent"];
	let missing_authority_result = &support::response_at(&responses, 1)["result"];
	let accept_result = &support::response_at(&responses, 2)["result"]["structuredContent"];
	let read_result = &support::response_at(&responses, 3)["result"]["contents"][0]["text"];
	let read_json: Value =
		serde_json::from_str(read_result.as_str().expect("resource text should parse"))
			.expect("resource should be json");

	assert_eq!(draft_result["schema"], "decodex.mcp.autonomy_objective_result/1");
	assert_eq!(draft_result["objective"]["state"], "draft");
	assert_eq!(draft_result["persisted"], true);
	assert_eq!(missing_authority_result["isError"], true);
	assert_eq!(missing_authority_result["structuredContent"]["reason"], "missing_authority");
	assert_eq!(accept_result["schema"], "decodex.mcp.autonomy_objective_result/1");
	assert_eq!(accept_result["objective"]["state"], "accepted");
	assert_eq!(accept_result["objective"]["acceptance_present"], true);
	assert_eq!(accept_result["authority_effect"], "accepted_objective_no_execution_authority");
	assert_eq!(read_json["objective"]["objective_id"], "self-iteration-pilot");
	assert_eq!(read_json["objective"]["state"], "accepted");
}

#[test]
fn autonomy_accept_objective_refuses_caller_supplied_runtime_policy_authority() {
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_autonomy_objective_draft("decodex", support::autonomy_objective_fixture())
		.expect("objective draft should persist");

	let responses = support::run_stdio_with_context(
		McpContext {
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(state_store),
		},
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"autonomy_accept_objective","arguments":{"mode":"apply","objectiveId":"quality-autonomy","objectiveVersion":1,"authority":{"acceptedBy":"policy:auto","acceptedByKind":"runtime_policy","acceptanceSource":"caller-supplied-policy"}}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
	assert_eq!(result["structuredContent"]["reason"], "objective_acceptance_refused");
	assert!(
		result["structuredContent"]["message"]
			.as_str()
			.expect("refusal message should be text")
			.contains("trusted Decodex authority state")
	);
}
