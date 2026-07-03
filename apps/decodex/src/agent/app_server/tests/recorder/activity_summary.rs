use crate::{
	agent::app_server::tests::recorder::{RunRecorder, TempDir},
	state::{self, StateStore},
};

#[test]
fn recorder_aggregates_child_agent_activity_breakdown() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let large_output = "x".repeat(100_500);
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder
		.record(
			"thread/status/changed",
			r#"{"method":"thread/status/changed","params":{"threadId":"thread-1","status":{"type":"active"}}}"#,
		)
		.expect("thread status should record");
	recorder
		.record(
			"item/tool/call",
			r#"{"method":"item/tool/call","params":{"tool":"functions.exec_command","arguments":{"cmd":"cargo make test"},"threadId":"thread-1","turnId":"turn-1","callId":"call-1"}}"#,
		)
		.expect("shell tool call should record");
	recorder
		.record(
			"item/tool/call/response",
			r#"{"contentItems":[{"type":"inputText","text":"tests passed"}],"success":true}"#,
		)
		.expect("shell tool response should record");

	for call_id in ["call-2", "call-3"] {
		recorder
			.record(
				"item/tool/call",
				&format!(
					r#"{{"method":"item/tool/call","params":{{"tool":"view_image","arguments":{{"detail":"original"}},"threadId":"thread-1","turnId":"turn-1","callId":"{call_id}"}}}}"#
				),
			)
			.expect("image tool call should record");
		recorder
			.record(
				"item/tool/call/response",
				&format!(
					r#"{{"contentItems":[{{"type":"inputText","text":"{large_output}"}}],"success":true}}"#
				),
			)
			.expect("image tool response should record");
	}

	recorder
		.record(
			"turn/completed",
			r#"{"method":"turn/completed","params":{"turn":{"id":"turn-1","status":"completed"},"usage":{"input_tokens":105000,"output_tokens":12000}}}"#,
		)
		.expect("turn completion should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.child_agent_activity().expect("child activity should be captured");
	let protocol_activity =
		marker.protocol_activity().expect("protocol activity should be captured");

	assert_eq!(summary.event_count, 8);
	assert_eq!(summary.tool_call_count, 3);
	assert_eq!(summary.current_bucket, None);
	assert_eq!(summary.input_tokens_current, Some(105_000));
	assert_eq!(summary.input_tokens_cumulative, 105_000);
	assert_eq!(summary.output_tokens_cumulative, 12_000);
	assert_eq!(summary.largest_tool_output_tool.as_deref(), Some("view_image"));
	assert!(
		summary
			.large_output_warnings
			.iter()
			.any(|warning| warning.contains("view_image repeated 2 large outputs"))
	);
	assert!(summary.buckets.iter().any(|bucket| {
		bucket.name == "Shell" && bucket.tool_call_count == 1 && bucket.event_count >= 2
	}));
	assert!(summary.buckets.iter().any(|bucket| {
		bucket.name == "Browser/Image"
			&& bucket.tool_call_count == 2
			&& bucket.output_bytes > 200_000
	}));
	assert!(summary.buckets.iter().any(|bucket| {
		bucket.name == "Model" && bucket.input_tokens == 105_000 && bucket.output_tokens == 12_000
	}));
	assert_eq!(protocol_activity.turn_status.as_deref(), Some("completed"));
	assert_eq!(protocol_activity.waiting_reason.as_deref(), Some("turn_completed"));
	assert_eq!(protocol_activity.recent_events.len(), 8);
	assert!(protocol_activity.recent_events.iter().any(|event| {
		event.event_type == "item/tool/call"
			&& event.detail.as_deref() == Some("functions.exec_command")
	}));
	assert!(protocol_activity.recent_events.iter().any(|event| {
		event.event_type == "turn/completed"
			&& event.category == "turn"
			&& event.detail.as_deref() == Some("completed")
	}));
}
#[test]
fn recorder_treats_matching_protocol_replay_as_idempotent() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let payload = r#"{"threadId":"thread-1","attemptNumber":5}"#;
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, None);

	recorder.record("thread/archive", payload).expect("archive event should record");

	recorder.next_sequence = 1;

	recorder
		.record("thread/archive", payload)
		.expect("matching protocol replay should not fail the app-server run");

	assert_eq!(state_store.event_count("run-1").expect("event count should load"), 1);
	assert_eq!(recorder.next_sequence, 2);
}
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
fn recorder_summarizes_v2_account_rate_limit_notifications() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder
		.record(
			"account/rateLimits/updated",
			r#"{"method":"account/rateLimits/updated","params":{"rateLimits":{"planType":"pro","rateLimitReachedType":"workspace_member_usage_limit_reached","primary":{"usedPercent":100}}}}"#,
		)
		.expect("rate limit protocol event should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");
	let event = summary.recent_events.first().expect("recent rate limit event should render");

	assert_eq!(summary.rate_limit_status.as_deref(), Some("workspace_member_usage_limit_reached"));
	assert_eq!(event.category, "rate_limit");
	assert_eq!(event.detail.as_deref(), Some("pro/workspace_member_usage_limit_reached"));
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
#[test]
fn recorder_does_not_treat_rate_limit_update_method_as_limit_status() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder
		.record(
			"account/rateLimits/updated",
			r#"{"method":"account/rateLimits/updated","params":{"rateLimits":{"planType":"pro","rateLimitReachedType":null,"primary":{"usedPercent":12}}}}"#,
		)
		.expect("rate limit update event should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");

	assert_eq!(summary.rate_limit_status, None);
	assert_eq!(
		summary.recent_events.first().and_then(|event| event.detail.as_deref()),
		Some("pro")
	);
}
#[test]
fn recorder_summarizes_wrapped_account_protocol_activity() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder
		.record(
			"account/update",
			r#"{"method":"account/update","params":{"planType":"pro","refreshStatus":"refreshed"}}"#,
		)
		.expect("account protocol event should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");
	let event = summary.recent_events.first().expect("recent account event should render");

	assert_eq!(event.category, "account");
	assert_eq!(event.detail.as_deref(), Some("pro/refreshed"));
}
