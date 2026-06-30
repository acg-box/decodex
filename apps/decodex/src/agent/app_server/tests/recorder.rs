#[allow(clippy::wildcard_imports)] use super::*;

#[test]
fn turn_notification_ignores_agent_output_for_non_target_turn() {
	let old_completed = JsonRpcNotification {
		method: String::from("item/completed"),
		params: serde_json::json!({
			"threadId": "thread-1",
			"turnId": "turn-old",
			"item": {"type": "agentMessage", "text": "OLD"}
		}),
	};
	let old_delta = JsonRpcNotification {
		method: String::from("item/agentMessage/delta"),
		params: serde_json::json!({
			"threadId": "thread-1",
			"turnId": "turn-old",
			"delta": " OLD_DELTA"
		}),
	};
	let target_completed = JsonRpcNotification {
		method: String::from("item/completed"),
		params: serde_json::json!({
			"threadId": "thread-1",
			"turnId": "turn-new",
			"item": {"type": "agentMessage", "text": "NEW"}
		}),
	};
	let mut final_output = String::from("CURRENT");
	let mut latest_turn_failure: Option<AppServerTurnFailure> = None;

	assert!(
		super::super::handle_turn_execution_notification(
			&old_completed,
			"thread-1",
			"turn-new",
			&mut final_output,
			&mut latest_turn_failure
		)
		.expect("old completed notification should parse")
		.is_none()
	);
	assert!(
		super::super::handle_turn_execution_notification(
			&old_delta,
			"thread-1",
			"turn-new",
			&mut final_output,
			&mut latest_turn_failure
		)
		.expect("old delta notification should parse")
		.is_none()
	);
	assert_eq!(final_output, "CURRENT");

	super::super::handle_turn_execution_notification(
		&target_completed,
		"thread-1",
		"turn-new",
		&mut final_output,
		&mut latest_turn_failure,
	)
	.expect("target completed notification should parse");

	assert_eq!(final_output, "NEW");
}

#[test]
fn dynamic_tool_call_unavailable_outside_turn_execution_is_protocol_diagnostic() {
	let dispatch =
		super::super::dynamic_tool_call_unavailable_for_phase(RequestWaitPhase::TurnStart);

	assert!(!dispatch.response.success);
	assert!(matches!(
		dispatch.response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("unavailable while waiting for turn/start")
	));
	assert_eq!(
		dispatch
			.terminal_failure
			.as_ref()
			.map(super::super::AppServerDynamicToolFailure::error_class),
		Some("app_server_dynamic_tool_protocol_failure")
	);

	let diagnostic = dispatch.diagnostic.expect("protocol failure should publish a diagnostic");

	assert_eq!(diagnostic.failure_class, "app_server_dynamic_tool_protocol_failure");
	assert!(diagnostic.message.contains("unavailable while waiting for turn/start"));
	assert_eq!(
		diagnostic.next_action,
		"inspect the declared dynamic tool surface and item/tool/call payload before retrying the lane"
	);
}

#[test]
fn interactive_request_updates_marker_turn_id_to_current_turn() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder.set_thread_id("thread-1").expect("thread marker should write");
	recorder.set_turn_id("turn-old").expect("initial turn marker should write");

	let request = JsonRpcRequest {
		id: serde_json::json!(1),
		method: String::from("item/tool/requestUserInput"),
		params: serde_json::json!({
			"threadId": "thread-1",
			"turnId": "turn-new",
		}),
	};

	super::super::record_interactive_request_state(&mut recorder, &request)
		.expect("interactive request state should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.thread_id(), Some("thread-1"));
	assert_eq!(marker.turn_id(), Some("turn-new"));
	assert_eq!(marker.thread_status(), Some("active"));
	assert_eq!(marker.thread_active_flags(), &[String::from("waitingOnUserInput")]);
}

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

#[test]
fn turn_execution_records_dynamic_tool_call_before_response() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let request = JsonRpcRequest {
		id: serde_json::json!(7),
		method: String::from("item/tool/call"),
		params: serde_json::json!({
			"tool": "issue_progress_checkpoint",
			"arguments": {"phase": "verifying"},
			"threadId": "thread-1",
			"turnId": "turn-1",
			"callId": "call-1",
		}),
	};
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	super::super::record_server_request(&mut recorder, &request)
		.expect("tool call request should record before handler execution");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let activity = marker.child_agent_activity().expect("child activity should be captured");

	assert_eq!(marker.last_event_type(), Some("item/tool/call"));
	assert_eq!(activity.current_bucket.as_deref(), Some("Tracker"));
	assert_eq!(activity.current_detail.as_deref(), Some("issue_progress_checkpoint"));
	assert!(activity.buckets.iter().any(|bucket| {
		bucket.name == "Tracker" && bucket.tool_call_count == 1 && bucket.event_count == 1
	}));
}

#[test]
#[ignore = "requires a live local codex app-server binary"]
fn live_app_server_resume_round_trip_updates_marker_and_state() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let marker_path = temp_dir.path().to_path_buf();
	let first_state_store = StateStore::open_in_memory().expect("state store should open");
	let handler = LiveResumeDynamicToolHandler;
	let guard = LiveResumeBoundaryGuard;
	let cwd = marker_path.display().to_string();
	let developer_instructions = String::from(
		"You are a live resume integration test. On the first turn, call the dynamic tool `echo_resume` exactly once with the JSON argument `{\"text\":\"FIRST_OK\"}` and then reply with the exact text CONTINUE. If the thread is later resumed and the user asks for `SECOND_OK`, call `echo_resume` exactly once with `{\"text\":\"SECOND_OK\"}` and then reply with the exact text DONE. Do not use shell. Do not inspect files.",
	);
	let first_result = super::super::execute_app_server_run(
		&super::super::AppServerRunRequest {
			project_id: String::from("test-project"),
			run_id: String::from("live-resume-run"),
			issue_id: String::from("live-resume-issue"),
			attempt_number: 1,
			listen: String::from("stdio://"),
			cwd: cwd.clone(),
			developer_instructions: developer_instructions.clone(),
			user_input: String::from(
				"Call `echo_resume` with `{\\\"text\\\":\\\"FIRST_OK\\\"}`. After the tool succeeds, reply with the exact text CONTINUE.",
			),
				max_turns: 3,
				timeout: Duration::from_secs(30),
				process_env: AppServerProcessEnv::default(),
				continuation_user_input: Some(String::from(
				"Call `echo_resume` with `{\\\"text\\\":\\\"SECOND_OK\\\"}`. After the tool succeeds, reply with the exact text DONE.",
			)),
			activity_marker_path: Some(marker_path.clone()),
			resume_thread_id: None,
			ephemeral_thread: false,
			command_exec_health_check: None,
				dynamic_tool_handler: Some(&handler),
				continuation_guard: Some(&guard),
				phase_goal_controller: None,
				codex_account_provider: None,
			},
		&first_state_store,
	)
	.expect("first live app-server run should succeed");

	assert!(first_result.continuation_pending);
	assert_eq!(first_result.final_output.trim(), "CONTINUE");

	let first_marker = state::read_run_activity_marker_snapshot(&marker_path)
		.expect("first marker snapshot should load")
		.expect("first marker snapshot should exist");

	assert_eq!(first_marker.run_id(), "live-resume-run");
	assert_eq!(first_marker.attempt_number(), 1);
	assert_eq!(first_marker.thread_id(), Some(first_result.thread_id.as_str()));
	assert_eq!(first_marker.turn_id(), Some(first_result.turn_id.as_str()));
	assert_eq!(first_marker.effective_cwd(), Some(cwd.as_str()));
	assert_eq!(first_marker.effective_approval_policy(), Some("never"));
	assert!(first_marker.last_protocol_activity_unix_epoch().is_some());

	let resumed_state_store =
		StateStore::open_in_memory().expect("resumed state store should open");
	let second_result = super::super::execute_app_server_run(
		&super::super::AppServerRunRequest {
			project_id: String::from("test-project"),
			run_id: String::from("live-resume-run"),
			issue_id: String::from("live-resume-issue"),
			attempt_number: 1,
			listen: String::from("stdio://"),
			cwd: cwd.clone(),
			developer_instructions,
			user_input: String::from(
				"Call `echo_resume` with `{\\\"text\\\":\\\"SECOND_OK\\\"}`. After the tool succeeds, reply with the exact text DONE.",
			),
				max_turns: 1,
				timeout: Duration::from_secs(30),
				process_env: AppServerProcessEnv::default(),
				continuation_user_input: None,
			activity_marker_path: Some(marker_path.clone()),
			resume_thread_id: Some(first_result.thread_id.clone()),
			ephemeral_thread: false,
			command_exec_health_check: None,
				dynamic_tool_handler: Some(&handler),
				continuation_guard: None,
				phase_goal_controller: None,
				codex_account_provider: None,
			},
		&resumed_state_store,
	)
	.expect("resumed live app-server run should succeed");

	assert!(!second_result.continuation_pending);
	assert_eq!(second_result.thread_id, first_result.thread_id);
	assert_ne!(second_result.turn_id, first_result.turn_id);
	assert_eq!(second_result.final_output.trim(), "DONE");

	let second_marker = state::read_run_activity_marker_snapshot(&marker_path)
		.expect("second marker snapshot should load")
		.expect("second marker snapshot should exist");

	assert_eq!(second_marker.thread_id(), Some(first_result.thread_id.as_str()));
	assert_eq!(second_marker.turn_id(), Some(second_result.turn_id.as_str()));
	assert_eq!(second_marker.effective_model_provider(), Some("openai"));
	assert_eq!(second_marker.effective_cwd(), Some(cwd.as_str()));
	assert!(second_marker.last_protocol_activity_unix_epoch().is_some());
	assert!(second_marker.event_count() > 0);

	let resumed_attempt = resumed_state_store
		.run_attempt("live-resume-run")
		.expect("resumed run attempt should load")
		.expect("resumed run attempt should exist");

	assert_eq!(resumed_attempt.thread_id(), Some(first_result.thread_id.as_str()));
	assert_eq!(resumed_attempt.turn_id(), Some(second_result.turn_id.as_str()));
}
