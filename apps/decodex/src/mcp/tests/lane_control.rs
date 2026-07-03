use serde_json::Value;

use crate::{
	mcp::{
		McpCapabilityProfile, McpContext,
		tests::support::{self},
	},
	runtime,
};

#[test]
fn tools_call_refuses_missing_plan_intent() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_plan","arguments":{}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.tool_validation_error/1");
	assert_eq!(result["structuredContent"]["reason"], "invalid_arguments");
	assert_eq!(result["structuredContent"]["tool"], "decodex_plan");
}

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

#[test]
fn tools_call_lane_control_steer_audits_and_queues_without_raw_message() {
	let repo = support::test_repo();
	let _runtime_home_guard = support::isolated_mcp_runtime_home(&repo);
	let config_path = repo.path().join("project.toml");

	support::seed_project_runtime_for_mcp_resources(repo.path(), &config_path);

	let responses = support::run_stdio_with_context(
		support::project_mcp_context(repo.path(), &config_path),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"steer","projectId":"pubfi","issue":"PUB-012","runId":"run-12","expectedTurnId":"turn-12","message":"Please stop after the current safe point.","authority":{"reason":"operator requested steer","source":"mcp-test","inspectedRunId":"run-12","expectedTurnId":"turn-12"}}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];
	let structured = &result["structuredContent"];
	let serialized = serde_json::to_string(structured).expect("structured should serialize");

	assert_eq!(result["isError"], false);
	assert_eq!(structured["status"], "queued");
	assert_eq!(structured["result"]["deliveryStatus"], "queued");
	assert_eq!(structured["result"]["messageLineCount"], 1);
	assert!(!serialized.contains("Please stop after the current safe point."));

	support::assert_no_sensitive_observability_content(structured);

	let state_store = runtime::open_runtime_store().expect("runtime store should open");
	let events = state_store
		.list_private_execution_events("pubfi", "PUB-012", "run-12", 1)
		.expect("private events should read");

	assert!(events.iter().any(|event| event.event_type() == "control_action"));
	assert!(events.iter().any(|event| event.event_type() == "lane_control/steer/requested"));
}

#[test]
fn tools_call_lane_control_soft_interrupt_accepts_and_force_requires_ack() {
	let repo = support::test_repo();
	let _runtime_home_guard = support::isolated_mcp_runtime_home(&repo);
	let config_path = repo.path().join("project.toml");

	support::seed_project_runtime_for_mcp_resources(repo.path(), &config_path);

	let force_refusal = support::run_stdio_with_context(
		support::project_mcp_context(repo.path(), &config_path),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"interrupt","projectId":"pubfi","issue":"PUB-012","runId":"run-12","force":true,"authority":{"reason":"operator requested hard fallback","source":"mcp-test","inspectedRunId":"run-12"}}}}"#,
	);
	let force_structured = &support::response_at(&force_refusal, 0)["result"]["structuredContent"];

	assert_eq!(force_structured["status"], "refused");
	assert_eq!(force_structured["reason"], "hard_fallback_authority_missing");

	let soft_acceptance = support::run_stdio_with_context(
		support::project_mcp_context(repo.path(), &config_path),
		r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"interrupt","projectId":"pubfi","issue":"PUB-012","runId":"run-12","authority":{"reason":"operator requested soft interrupt","source":"mcp-test","inspectedRunId":"run-12"}}}}"#,
	);
	let soft_result = &support::response_at(&soft_acceptance, 0)["result"];
	let soft_structured = &soft_result["structuredContent"];

	assert_eq!(soft_result["isError"], false);
	assert_eq!(soft_structured["status"], "queued");
	assert_eq!(
		soft_structured["result"]["softInterrupt"]["classification"],
		"soft_interrupt_pending"
	);
	assert_eq!(soft_structured["result"]["hardInterrupt"], Value::Null);

	support::assert_no_sensitive_observability_content(soft_structured);
}

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

#[test]
fn tools_call_returns_structured_refusal_for_invalid_observe_arguments() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_observe","arguments":{"limit":0}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["status"], "refused");
	assert_eq!(result["structuredContent"]["reason"], "invalid_limit");
}

#[test]
fn tools_call_emits_json_rpc_progress_notification_when_requested() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"},"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}"#,
	);

	assert_eq!(responses[0]["method"], "notifications/progress");
	assert_eq!(responses[0]["params"]["progressToken"], "progress-1");
	assert_eq!(responses[1]["result"]["structuredContent"]["status"], "ok");
}

#[test]
fn tools_call_does_not_emit_progress_for_invalid_params() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"}}}"#,
	);

	assert_eq!(responses.len(), 1);
	assert_eq!(responses[0]["id"], 1);
	assert_eq!(responses[0]["error"]["code"], -32_602);
}

#[test]
fn tools_call_does_not_emit_progress_for_structured_validation_error() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"},"name":"decodex_plan","arguments":{}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(responses.len(), 1);
	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.tool_validation_error/1");
}

#[test]
fn tools_call_does_not_emit_progress_for_structured_refusal() {
	let repo = support::test_repo();
	let responses = support::run_stdio_with_profile(
		repo.path(),
		McpCapabilityProfile::Observe,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"},"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(responses.len(), 1);
	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
	assert_eq!(result["structuredContent"]["reason"], "insufficient_capability_profile");
}
