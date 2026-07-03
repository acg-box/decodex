use std::{cell::RefCell, fs, time::Duration};

use tempfile::TempDir;

use crate::{
	agent::{
		app_server::tests::{
			LaneControlSteerRequest, LaneControlSteerRequestInput, LaneControlSteerResponse,
			NamespacedDynamicToolHandler, Result,
		},
		json_rpc::JsonRpcRequest,
		tracker_tool_bridge::DynamicToolContentItem,
	},
	prelude::eyre,
	run_control,
};

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
					.map(super::AppServerDynamicToolFailure::error_class),
				Some("app_server_dynamic_tool_protocol_failure"),
				"{case_name}"
			);
		} else {
			assert!(dispatch.terminal_failure.is_none(), "{case_name}");
		}
	}
}
