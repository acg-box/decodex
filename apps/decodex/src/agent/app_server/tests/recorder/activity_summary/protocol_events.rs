use crate::{
	agent::app_server::tests::recorder::{RunRecorder, TempDir},
	state::{self, StateStore},
};

#[test]
fn recorder_summarizes_high_value_protocol_activity() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	for (event_type, payload) in [
		(
			"turn/started",
			r#"{"method":"turn/started","params":{"turn":{"id":"turn-1","status":"running"}}}"#,
		),
		("plan/update", r#"{"method":"plan/update","params":{"step":"verify"}}"#),
		("diff/update", r#"{"method":"diff/update","params":{"filesChanged":2}}"#),
		(
			"item/tool/call/failure",
			r#"{"failureClass":"app_server_dynamic_tool_failed","tool":"issue_comment","message":"tool rejected","nextAction":"retry"}"#,
		),
		("command/output/delta", r#"{"method":"command/output/delta","params":{"delta":"ok"}}"#),
		("item/tool/requestUserInput/response", r#"{"answers":{}}"#),
		(
			"item/tool/requestUserInput",
			r#"{"method":"item/tool/requestUserInput","params":{"threadId":"thread-1","turnId":"turn-1"}}"#,
		),
		(
			"account/rateLimit/update",
			r#"{"rateLimitReachedType":"primary","primaryRemainingPercent":0}"#,
		),
	] {
		recorder.record(event_type, payload).expect("protocol event should record");
	}

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");
	let categories =
		summary.recent_events.iter().map(|event| event.category.as_str()).collect::<Vec<_>>();

	assert_eq!(summary.turn_status.as_deref(), Some("running"));
	assert_eq!(summary.waiting_reason.as_deref(), Some("approval_or_user_input"));
	assert_eq!(summary.rate_limit_status.as_deref(), Some("primary"));
	assert!(categories.contains(&"turn"));
	assert!(categories.contains(&"plan"));
	assert!(categories.contains(&"diff"));
	assert!(categories.contains(&"item"));
	assert!(categories.contains(&"command_output"));
	assert!(categories.contains(&"protocol_error"));
	assert!(categories.contains(&"server_request_resolution"));
	assert!(categories.contains(&"rate_limit"));
	assert!(summary.recent_events.iter().any(|event| {
		event.event_type == "item/tool/call/failure"
			&& event.detail.as_deref() == Some("app_server_dynamic_tool_failed")
	}));
}

#[test]
fn recorder_summarizes_codex_app_server_warning_and_model_notifications() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	for (event_type, payload) in [
		(
			"deprecationNotice",
			r#"{"method":"deprecationNotice","params":{"summary":"persistExtendedHistory is ignored","details":"Remove the request field."}}"#,
		),
		(
			"configWarning",
			r#"{"method":"configWarning","params":{"summary":"unknown feature key in config","details":"builtin_mcp"}}"#,
		),
		(
			"model/rerouted",
			r#"{"method":"model/rerouted","params":{"threadId":"thread-1","turnId":"turn-1","fromModel":"gpt-5.4","toModel":"gpt-5.5","reason":"highRiskCyberActivity"}}"#,
		),
		(
			"model/verification",
			r#"{"method":"model/verification","params":{"threadId":"thread-1","turnId":"turn-1","verifications":["trustedAccessForCyber"]}}"#,
		),
		(
			"thread/tokenUsage/updated",
			r#"{"method":"thread/tokenUsage/updated","params":{"threadId":"thread-1","turnId":"turn-1","tokenUsage":{"last":{"inputTokens":10,"cachedInputTokens":0,"outputTokens":5,"reasoningOutputTokens":1,"totalTokens":16},"total":{"inputTokens":100,"cachedInputTokens":12,"outputTokens":30,"reasoningOutputTokens":8,"totalTokens":138},"modelContextWindow":200000}}}"#,
		),
	] {
		recorder.record(event_type, payload).expect("protocol event should record");
	}

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");
	let categories =
		summary.recent_events.iter().map(|event| event.category.as_str()).collect::<Vec<_>>();

	assert!(categories.contains(&"deprecation"));
	assert!(categories.contains(&"warning"));
	assert!(categories.contains(&"model"));
	assert!(categories.contains(&"token_usage"));
	assert!(summary.recent_events.iter().any(|event| {
		event.event_type == "deprecationNotice"
			&& event.detail.as_deref() == Some("persistExtendedHistory is ignored")
	}));
	assert!(summary.recent_events.iter().any(|event| {
		event.event_type == "model/rerouted"
			&& event.detail.as_deref() == Some("gpt-5.4->gpt-5.5/highRiskCyberActivity")
	}));
	assert!(summary.recent_events.iter().any(|event| {
		event.event_type == "thread/tokenUsage/updated"
			&& event.detail.as_deref() == Some("input=100, output=30")
	}));
}
