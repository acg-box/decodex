use crate::{
	agent::app_server::tests::recorder::{
		AppServerProcessEnv, AppServerRunRequest, AppServerTurnFailure, Duration,
		JsonRpcNotification, JsonRpcRequest, LiveResumeBoundaryGuard, LiveResumeDynamicToolHandler,
		RunRecorder, TempDir,
	},
	state::{self, StateStore},
};

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
		super::handle_turn_execution_notification(
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
		super::handle_turn_execution_notification(
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

	super::handle_turn_execution_notification(
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

	super::record_interactive_request_state(&mut recorder, &request)
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
	let first_result = super::execute_app_server_run(
		&AppServerRunRequest {
			project_id: String::from("test-project"),
			run_id: String::from("live-resume-run"),
			issue_id: String::from("live-resume-issue"),
			issue_identifier: String::from("XY-1"),
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
	let second_result = super::execute_app_server_run(
		&AppServerRunRequest {
			project_id: String::from("test-project"),
			run_id: String::from("live-resume-run"),
			issue_id: String::from("live-resume-issue"),
			issue_identifier: String::from("XY-1"),
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
