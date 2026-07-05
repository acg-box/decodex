use serde_json::Value;

use crate::{
	mcp::tests::support::{self},
	runtime,
};

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
