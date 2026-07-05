use std::{
	io::Write as _,
	process::{Command, Stdio},
};

use serde_json::Value;

use crate::mcp_stdio::{http, project, support::ChildGuard};

#[test]
fn mcp_stdio_process_stdout_contains_only_json_rpc() {
	let repo = project::test_repo();
	let mut child = Command::new(env!("CARGO_BIN_EXE_decodex"))
		.args(["mcp", "serve", "--transport", "stdio"])
		.current_dir(repo.path())
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.expect("decodex mcp process should spawn");

	{
		let stdin = child.stdin.as_mut().expect("child stdin should be open");

		stdin
			.write_all(
				br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}
{"jsonrpc":"2.0","id":2,"method":"resources/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"resources/templates/list","params":{}}
{"jsonrpc":"2.0","id":4,"method":"prompts/list","params":{}}
{"jsonrpc":"2.0","id":5,"method":"prompts/get","params":{"name":"decodex_validation_ready","arguments":{"issue":"XY-994"}}}
{"jsonrpc":"2.0","id":6,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}
"#,
			)
			.expect("stdio request should write");
	}

	drop(child.stdin.take());

	let output = child.wait_with_output().expect("child should exit");

	assert!(output.status.success(), "mcp process failed: {:?}", output.status);
	assert!(
		String::from_utf8_lossy(&output.stderr).trim().is_empty(),
		"mcp process should not print diagnostics for the smoke path"
	);

	let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
	let lines = stdout.lines().collect::<Vec<_>>();

	assert_eq!(lines.len(), 7);

	for line in lines {
		let value = serde_json::from_str::<Value>(line).expect("stdout line should be JSON");

		assert_eq!(value["jsonrpc"], "2.0");
	}
}

#[test]
fn mcp_streamable_http_process_observe_profile_smoke() {
	let fixture = project::test_project();
	let addr = http::free_loopback_address();
	let mut child = ChildGuard::new(
		Command::new(env!("CARGO_BIN_EXE_decodex"))
			.args([
				"mcp",
				"serve",
				"--config",
				fixture.config_path.to_str().expect("config path should be utf-8"),
				"--transport",
				"streamable-http",
				"--listen-address",
				addr.as_str(),
			])
			.current_dir(&fixture.repo_path)
			.env("HOME", fixture.home_path.to_str().expect("home path should be utf-8"))
			.stdin(Stdio::null())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.expect("decodex mcp HTTP process should spawn"),
	);

	http::wait_for_streamable_http(&addr, &mut child);

	let origin = format!("http://{addr}");
	let initialize = http::http_post(
		&addr,
		&[("Origin", origin.as_str()), ("Accept", "application/json")],
		r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
	);
	let session_id = initialize.header("mcp-session-id").expect("session id").to_owned();

	assert_eq!(initialize.status(), "HTTP/1.1 200 OK");
	assert_eq!(initialize.json_body()["result"]["protocolVersion"], "2025-11-25");

	let tools_list = http::http_post(
		&addr,
		&[
			("Origin", origin.as_str()),
			("Accept", "application/json"),
			("Mcp-Session-Id", session_id.as_str()),
		],
		r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
	);
	let tools_list_body = tools_list.json_body();
	let tool_names = tools_list_body["result"]["tools"]
		.as_array()
		.expect("tools array")
		.iter()
		.filter_map(|tool| tool.get("name").and_then(Value::as_str))
		.collect::<Vec<_>>();

	assert_eq!(tool_names, vec!["decodex_observe"]);

	let above_profile = http::http_post(
		&addr,
		&[
			("Origin", origin.as_str()),
			("Accept", "application/json"),
			("Mcp-Session-Id", session_id.as_str()),
		],
		r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}"#,
	);
	let above_profile_body = above_profile.json_body();

	assert_eq!(
		above_profile_body["result"]["structuredContent"]["reason"],
		"insufficient_capability_profile"
	);
	assert_eq!(above_profile_body["result"]["structuredContent"]["capability_profile"], "observe");

	let observe_sse = http::http_post(
		&addr,
		&[
			("Origin", origin.as_str()),
			("Accept", "text/event-stream"),
			("Mcp-Session-Id", session_id.as_str()),
		],
		r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"},"name":"decodex_observe","arguments":{"limit":1}}}"#,
	);
	let observe_sse_body = observe_sse.body_text();

	assert_eq!(observe_sse.status(), "HTTP/1.1 200 OK");
	assert_eq!(observe_sse.header("content-type"), Some("text/event-stream"));
	assert!(observe_sse_body.contains("event: message"));
	assert!(observe_sse_body.contains("\"method\":\"notifications/progress\""));
	assert!(observe_sse_body.contains("\"progressToken\":\"progress-1\""));
	assert!(observe_sse_body.contains("\"id\":4"));

	let output = child.stop();

	assert!(output.stdout.is_empty(), "HTTP MCP process must not write stdout");
	assert!(
		String::from_utf8_lossy(&output.stderr).trim().is_empty(),
		"HTTP MCP process should not print diagnostics for the smoke path"
	);
}
