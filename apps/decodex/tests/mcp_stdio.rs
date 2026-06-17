//! Process-level smoke tests for the Decodex MCP stdio gateway.

#![allow(unused_crate_dependencies)]

use std::{
	fs,
	io::Write as _,
	path::PathBuf,
	process::{Command, Stdio},
};

use serde_json::Value;
use tempfile::TempDir;

#[test]
fn mcp_stdio_process_stdout_contains_only_json_rpc() {
	let repo = test_repo();
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

fn test_repo() -> TempDir {
	let repo = TempDir::new().expect("temp repo should exist");

	write_file(repo.path().join("Cargo.toml"), "[workspace]\n");
	write_file(repo.path().join("docs/index.md"), "# Docs\n");
	write_file(repo.path().join("docs/policy.md"), "# Policy\n");
	write_file(repo.path().join("docs/spec/runtime.md"), "# Runtime\n");

	repo
}

fn write_file(path: PathBuf, contents: &str) {
	let parent = path.parent().expect("test path should have parent");

	fs::create_dir_all(parent).expect("parent directory should exist");
	fs::write(path, contents).expect("test file should write");
}
