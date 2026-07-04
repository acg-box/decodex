use std::{
	collections::VecDeque,
	process::{Command, Stdio},
	sync::{Arc, Mutex, mpsc},
	time::Duration,
};

use color_eyre::Report;
use serde_json::Value;

use crate::agent::json_rpc::{
	AppServerOutputTimeout, AppServerTransportFailure, JsonRpcConnection, tests::support,
};

#[test]
fn request_wait_ignores_orphan_response_before_expected_response() {
	let mut connection = support::test_connection_with_messages([
		r#"{"id":99,"result":{"late":true}}"#,
		r#"{"id":1,"result":{"ok":true}}"#,
	]);
	let response: Value = connection
		.request_with_handler(
			"thread/start",
			&serde_json::json!({}),
			Duration::from_secs(1),
			|_, _, _| Ok(()),
		)
		.expect("orphan response should not fail the pending request");

	assert_eq!(response, serde_json::json!({"ok": true}));
}

#[test]
fn stdin_write_failures_classify_as_app_server_transport_failures() {
	let mut child = Command::new("sh")
		.args(["-c", "exit 17"])
		.stdin(Stdio::piped())
		.stdout(Stdio::null())
		.stderr(Stdio::null())
		.spawn()
		.expect("child process should spawn");
	let stdin = child.stdin.take().expect("child stdin should be captured");
	let _status = child.wait().expect("child should exit");
	let (_stdout_tx, stdout_rx) = mpsc::channel();
	let stderr_tail =
		Arc::new(Mutex::new(VecDeque::from([String::from("fatal app-server transport test")])));
	let mut connection = JsonRpcConnection {
		child,
		stdin,
		stdout_rx,
		stderr_tail,
		pending_messages: VecDeque::new(),
		next_request_id: 1,
	};
	let error = connection
		.notify::<Value>("thread/test", None)
		.expect_err("closed stdin should fail as transport");

	assert!(error.downcast_ref::<AppServerTransportFailure>().is_some());
	assert!(error.to_string().contains("App-server stdin write failed"));
	assert!(error.to_string().contains("fatal app-server transport test"));
}

#[test]
fn output_timeouts_downcast_to_timeout_class() {
	let error = Report::new(AppServerOutputTimeout);

	assert!(error.downcast_ref::<AppServerOutputTimeout>().is_some());
	assert_eq!(error.to_string(), "Timed out while waiting for app-server output.");
}

#[test]
fn wrapped_transport_failures_still_downcast_to_transport_class() {
	let error = Report::new(AppServerTransportFailure::new(String::from(
		"App-server stdout disconnected unexpectedly.",
	)))
	.wrap_err("outer context");

	assert!(error.downcast_ref::<AppServerTransportFailure>().is_some());
}
