use std::{
	cell::RefCell,
	fs,
	path::PathBuf,
	time::{Duration, Instant},
};

use tempfile::TempDir;

use crate::{
	agent::{
		app_server::tests::{
			AppServerHomePreflightFailure, AppServerTurnFailure, ContinuingCompletionHandler,
			EffectiveThreadConfig, InitializeResponse, JsonRpcError, JsonRpcErrorPayload,
			LaneControlSteerRequest, LaneControlSteerRequestInput, LaneControlSteerResponse,
			NamespacedDynamicToolHandler, ProbeDynamicToolHandler, RejectingCompletionHandler,
			RejectingContinuationGuard, ResolvedAppServerCodexHomeEnv, Result, RunRecorder,
			TurnCompletionStatus, YieldingContinuationGuard,
		},
		json_rpc::{JsonRpcNotification, JsonRpcRequest},
		tracker_tool_bridge::DynamicToolContentItem,
	},
	prelude::eyre,
	run_control,
	state::{ProtocolActivitySummary, StateStore},
};

#[test]
fn remaining_idle_budget_resets_from_latest_activity() {
	let now = Instant::now();
	let timeout = Duration::from_secs(300);
	let last_activity_at = now.checked_sub(Duration::from_secs(12)).expect("instant math");
	let remaining =
		super::remaining_idle_budget(last_activity_at, now, timeout).expect("budget should remain");

	assert!(remaining <= timeout);
	assert!(remaining >= Duration::from_secs(287));
}

#[test]
fn remaining_idle_budget_expires_after_idle_timeout() {
	let now = Instant::now();
	let timeout = Duration::from_secs(300);
	let last_activity_at = now.checked_sub(Duration::from_secs(301)).expect("instant math");

	assert!(super::remaining_idle_budget(last_activity_at, now, timeout).is_none());
}

#[test]
fn protocol_activity_idle_timeout_extends_running_model_execution() {
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model_execution")),
		..ProtocolActivitySummary::default()
	};

	assert_eq!(
		super::protocol_activity_idle_timeout(
			Some(&protocol_activity),
			super::super::RUN_LEASE_IDLE_TIMEOUT
		),
		super::super::MODEL_EXECUTION_IDLE_TIMEOUT
	);
}

#[test]
fn protocol_activity_idle_timeout_keeps_base_timeout_for_other_waits() {
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("tool_execution")),
		..ProtocolActivitySummary::default()
	};

	assert_eq!(
		super::protocol_activity_idle_timeout(
			Some(&protocol_activity),
			super::super::RUN_LEASE_IDLE_TIMEOUT
		),
		super::super::RUN_LEASE_IDLE_TIMEOUT
	);
}

#[test]
fn run_recorder_keeps_events_when_marker_write_fails() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let missing_worktree = PathBuf::from(temp_dir.path()).join("missing-worktree");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&missing_worktree));

	recorder.mark_activity().expect("marker failures should be non-fatal");
	recorder.record("turn/started", "{\"turn\":\"1\"}").expect("event should record");

	assert_eq!(state_store.event_count("run-1").expect("event count should load"), 1);
}

#[test]
fn completion_classification_uses_dynamic_tool_handler() {
	let error = super::classify_turn_completion(Some(&RejectingCompletionHandler), "finished")
		.expect_err("completion classifier should be consulted");

	assert!(error.to_string().contains("terminal finalization missing"));
}

#[test]
fn completion_classification_defaults_to_complete_without_handler() {
	assert_eq!(
		super::classify_turn_completion(None, "finished")
			.expect("missing dynamic handler should not fail completion"),
		TurnCompletionStatus::Complete
	);
}

#[test]
fn probe_handler_allows_completion_classification() {
	assert_eq!(
		super::classify_turn_completion(Some(&ProbeDynamicToolHandler), "PROBE_OK")
			.expect("probe handler should not override completion validation"),
		TurnCompletionStatus::Complete
	);
}

#[test]
fn nonterminal_single_turn_completion_stays_invalid() {
	let error = super::reject_nonterminal_single_turn_completion(
		Some(&ContinuingCompletionHandler),
		"unfinished",
	)
	.expect_err("single-turn mode should preserve terminal completion validation");

	assert!(error.to_string().contains("terminal finalization missing"));
}

#[test]
fn continuation_boundary_reached_yields_when_guard_allows_it() {
	assert!(
		super::continuation_boundary_reached(Some(&YieldingContinuationGuard), 2)
			.expect("yielding guard should allow a clean continuation boundary")
	);
}

#[test]
fn continuation_boundary_reached_rejects_invalid_boundary() {
	let error = super::continuation_boundary_reached(Some(&RejectingContinuationGuard), 1)
		.expect_err("invalid continuation boundaries should surface as errors");

	assert!(error.to_string().contains("turn 1 hit an invalid continuation boundary"));
}

#[test]
fn validate_effective_thread_config_accepts_noninteractive_runtime() {
	let runtime = EffectiveThreadConfig {
		model: String::from("gpt-5.4"),
		model_provider: String::from("openai"),
		cwd: String::from("/tmp/worktree"),
		approval_policy: String::from("never"),
		approvals_reviewer: String::from("human"),
		sandbox_mode: String::from("workspaceWrite"),
	};

	super::validate_effective_thread_config("/tmp/worktree", &runtime)
		.expect("matching non-interactive config should validate");
}

#[test]
fn validate_effective_thread_config_rejects_interactive_runtime_policies() {
	for (case_name, approval_policy, sandbox_mode, expected) in [
		(
			"interactive approval policy",
			"onRequest",
			"workspaceWrite",
			"approval policy `onRequest`",
		),
		("read-only sandbox", "never", "readOnly", "readOnly"),
	] {
		let runtime = EffectiveThreadConfig {
			model: String::from("gpt-5.4"),
			model_provider: String::from("openai"),
			cwd: String::from("/tmp/worktree"),
			approval_policy: String::from(approval_policy),
			approvals_reviewer: String::from("human"),
			sandbox_mode: String::from(sandbox_mode),
		};
		let error = super::validate_effective_thread_config("/tmp/worktree", &runtime)
			.expect_err(case_name);

		assert!(
			error.to_string().contains(expected),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}

#[test]
fn initialize_codex_home_assertion_accepts_expected_home() {
	let expected = ResolvedAppServerCodexHomeEnv::new(
		PathBuf::from("/Users/test/.codex"),
		PathBuf::from("/Users/test/.codex"),
	)
	.expect("test Codex home should validate");
	let response = InitializeResponse {
		user_agent: String::from("codex-cli-test"),
		codex_home: String::from("/Users/test/.codex"),
		platform_family: String::from("unix"),
		platform_os: String::from("macos"),
	};

	super::validate_initialize_codex_home(&expected, &response)
		.expect("matching Codex home should pass");
}

#[test]
fn initialize_codex_home_assertion_blocks_before_thread_start_on_mismatch() {
	let expected = ResolvedAppServerCodexHomeEnv::new(
		PathBuf::from("/Users/test/.codex"),
		PathBuf::from("/Users/test/.codex"),
	)
	.expect("test Codex home should validate");
	let response = InitializeResponse {
		user_agent: String::from("codex-cli-test"),
		codex_home: String::from("/tmp/per-account-codex-home"),
		platform_family: String::from("unix"),
		platform_os: String::from("macos"),
	};
	let error = super::validate_initialize_codex_home(&expected, &response)
		.expect_err("mismatched Codex home should fail before thread start");

	assert!(error.downcast_ref::<AppServerHomePreflightFailure>().is_some());
	assert!(error.to_string().contains("initialize codexHome `/tmp/per-account-codex-home`"));
	assert!(error.to_string().contains("expected shared Codex home `/Users/test/.codex`"));
	assert!(error.to_string().contains("before thread/start"));
}

#[test]
fn app_server_turn_failure_classifies_operator_attention() {
	for (case_name, message, code, requires_attention, error_class) in [
		(
			"usage limit",
			"You've hit your usage limit.",
			Some(String::from("usageLimitExceeded")),
			false,
			"app_server_usage_limit_exceeded",
		),
		("generic failure", "transient model failure", None, false, "app_server_turn_failed"),
	] {
		let failure = AppServerTurnFailure::new(
			"thread-1",
			Some(String::from("turn-1")),
			"failed",
			message,
			code.clone(),
		);

		assert_eq!(failure.requires_operator_attention(), requires_attention, "{case_name}");
		assert_eq!(failure.error_class(), error_class);
		assert_eq!(failure.should_stop_current_turn(), case_name == "usage limit", "{case_name}");

		if let Some(code) = code {
			assert!(failure.to_string().contains(&code));
		}
	}
}

#[test]
fn structured_error_notification_becomes_turn_failure() {
	let notification = JsonRpcNotification {
		method: String::from("error"),
		params: serde_json::json!({
			"error": {
				"message": {
					"kind": "protocolFailure",
					"detail": "unexpected response"
				},
				"codexErrorInfo": {
					"type": "appServerProtocolMismatch"
				}
			},
			"threadId": "thread-1",
			"turnId": "turn-1",
			"willRetry": false
		}),
	};
	let (failure, will_retry) =
		super::failure_from_error_notification(&notification, "thread-1", "turn-1")
			.expect("structured error payload should parse")
			.expect("matching error notification should produce a failure");
	let failure_message = failure.to_string();

	assert!(failure_message.contains("protocolFailure"));
	assert!(failure_message.contains("appServerProtocolMismatch"));
	assert_eq!(will_retry, Some(false));
}

#[test]
fn json_rpc_error_response_becomes_recoverable_turn_failure() {
	let error = JsonRpcError {
		id: serde_json::json!(7),
		error: JsonRpcErrorPayload {
			code: -32_000,
			message: String::from("late response"),
			data: None,
		},
	};
	let failure = super::turn_failure_from_json_rpc_error_response("thread-1", "turn-1", &error);
	let failure_message = failure.to_string();

	assert!(failure_message.contains("thread-1"));
	assert!(failure_message.contains("turn-1"));
	assert!(failure_message.contains("code -32000"));
	assert!(failure_message.contains("late response"));
}

#[test]
fn steer_delivery_error_classifies_active_turn_not_steerable_distinctly() {
	let error = eyre::eyre!(
		"`turn/steer` failed with -32000: turn is not steerable data: {{\"type\":\"activeTurnNotSteerable\"}}"
	);
	let failure_class = super::steer_error_class(&error);

	assert_eq!(failure_class, "active_turn_not_steerable");
}

#[test]
fn steer_delivery_error_classifies_missing_method_as_unsupported() {
	for message in [
		"`turn/steer` failed with -32601: method not found",
		"`turn/steer` failed with -32601: Method not found",
	] {
		let error = eyre::eyre!("{message}");
		let failure_class = super::steer_error_class(&error);

		assert_eq!(failure_class, "app_server_turn_steer_unsupported");
	}
}

#[test]
fn steer_response_wait_ignores_temp_file_until_atomic_response_exists() -> Result<()> {
	let temp_dir = TempDir::new()?;
	let request = LaneControlSteerRequest::new(LaneControlSteerRequestInput {
		audit_record_id: 7,
		project_id: "decodex",
		issue_id: "XY-704",
		run_id: "run-1",
		attempt_number: 1,
		thread_id: "thread-1",
		expected_turn_id: "turn-1",
		source: "test",
		message: "change direction",
	});
	let run_dir = temp_dir.path().join(".decodex-run-control").join("run-1");

	fs::create_dir_all(&run_dir)?;
	fs::write(run_dir.join(format!("{}.steer-response.json.tmp", request.request_id)), b"{")?;

	assert!(
		run_control::wait_for_steer_response(
			temp_dir.path(),
			"run-1",
			&request.request_id,
			Duration::from_millis(1),
		)?
		.is_none()
	);

	let response = LaneControlSteerResponse::delivered(&request, "turn-1", "turn-2");

	run_control::write_steer_response(temp_dir.path(), &response)?;

	assert_eq!(
		run_control::wait_for_steer_response(
			temp_dir.path(),
			"run-1",
			&request.request_id,
			Duration::from_millis(100),
		)?,
		Some(response)
	);

	Ok(())
}

#[test]
fn thread_resume_fallback_only_allows_missing_thread_errors() {
	assert!(super::thread_resume_error_allows_fallback(&eyre::eyre!("thread not found")));
	assert!(super::thread_resume_error_allows_fallback(&eyre::eyre!(
		"no rollout found for thread id thread-1"
	)));
	assert!(!super::thread_resume_error_allows_fallback(&eyre::eyre!(
		"failed to load rollout from disk"
	)));
	assert!(!super::thread_resume_error_allows_fallback(&eyre::eyre!(
		"thread belongs to another cwd"
	)));
}

#[test]
fn dynamic_tool_call_enforces_declared_namespace() {
	for (case_name, namespace, expected_success, expected_seen_namespace, expected_error) in [
		(
			"unknown namespace",
			Some("other"),
			false,
			None,
			Some(
				"Dynamic tool `tracker_tool` was called under namespace `other`, but this run did not declare that tool namespace.",
			),
		),
		("declared namespace", Some("tracker"), true, Some("tracker"), None),
		(
			"missing namespace",
			None,
			false,
			None,
			Some("Dynamic tool `tracker_tool` is not declared for this run attempt."),
		),
	] {
		let handler = NamespacedDynamicToolHandler { seen_namespace: RefCell::new(None) };
		let mut params = serde_json::json!({
			"arguments": {},
			"callId": "call-1",
			"threadId": "thread-1",
			"tool": "tracker_tool",
			"turnId": "turn-1"
		});

		if let Some(namespace) = namespace {
			params["namespace"] = serde_json::json!(namespace);
		}

		let request = JsonRpcRequest {
			id: serde_json::json!(1),
			method: String::from("item/tool/call"),
			params,
		};
		let dispatch =
			super::handle_dynamic_tool_call(Some(&handler), &request, "thread-1", Some("turn-1"));

		assert_eq!(dispatch.response.success, expected_success, "{case_name}");
		assert_eq!(
			*handler.seen_namespace.borrow(),
			expected_seen_namespace.map(String::from),
			"{case_name}"
		);

		if let Some(expected_error) = expected_error {
			assert_eq!(
				dispatch.response.content_items,
				vec![DynamicToolContentItem::InputText { text: String::from(expected_error) }],
				"{case_name}"
			);
			assert_eq!(
				dispatch
					.terminal_failure
					.as_ref()
					.map(super::super::AppServerDynamicToolFailure::error_class),
				Some("app_server_dynamic_tool_protocol_failure"),
				"{case_name}"
			);
		} else {
			assert!(dispatch.terminal_failure.is_none(), "{case_name}");
		}
	}
}
