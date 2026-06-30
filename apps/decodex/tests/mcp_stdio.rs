//! Process-level smoke tests for the Decodex MCP stdio gateway.

#![allow(unused_crate_dependencies)]

use std::{
	fs,
	io::{Read as _, Result, Write},
	net::{Shutdown, TcpListener, TcpStream},
	path::{Path, PathBuf},
	process::{Child, Command, ExitStatus, Output, Stdio},
	str, thread,
	time::{Duration, Instant},
};

use serde_json::Value;
use tempfile::TempDir;

struct TestProject {
	_home: TempDir,
	_project: TempDir,
	repo_path: PathBuf,
	home_path: PathBuf,
	config_path: PathBuf,
}

struct ChildGuard {
	child: Option<Child>,
}
impl ChildGuard {
	fn new(child: Child) -> Self {
		Self { child: Some(child) }
	}

	fn try_wait(&mut self) -> Option<ExitStatus> {
		self.child.as_mut().and_then(|child| child.try_wait().expect("child wait should run"))
	}

	fn stop(mut self) -> Output {
		let mut child = self.child.take().expect("child should exist");
		let _ = child.kill();

		child.wait_with_output().expect("child output should collect")
	}
}

impl Drop for ChildGuard {
	fn drop(&mut self) {
		if let Some(child) = self.child.as_mut() {
			let _ = child.kill();
			let _ = child.wait();
		}
	}
}

#[derive(Debug)]
struct ParsedHttpResponse {
	status: String,
	headers: Vec<(String, String)>,
	body: Vec<u8>,
}
impl ParsedHttpResponse {
	fn parse(bytes: Vec<u8>) -> Self {
		let header_end = bytes
			.windows(4)
			.position(|window| window == b"\r\n\r\n")
			.expect("HTTP response should have headers");
		let header_text = str::from_utf8(&bytes[..header_end]).expect("headers should be utf-8");
		let mut lines = header_text.split("\r\n");
		let status = lines.next().expect("status line should exist").to_owned();
		let headers = lines
			.filter_map(|line| {
				let (name, value) = line.split_once(':')?;

				Some((name.trim().to_ascii_lowercase(), value.trim().to_owned()))
			})
			.collect();

		Self { status, headers, body: bytes[header_end + 4..].to_vec() }
	}

	fn header(&self, name: &str) -> Option<&str> {
		let lower_name = name.to_ascii_lowercase();

		self.headers
			.iter()
			.find(|(header, _)| header == &lower_name)
			.map(|(_, value)| value.as_str())
	}

	fn body_text(&self) -> String {
		String::from_utf8(self.body.clone()).expect("body should be utf-8")
	}

	fn json_body(&self) -> Value {
		serde_json::from_slice(&self.body).expect("body should be JSON")
	}
}

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

#[test]
fn mcp_streamable_http_process_observe_profile_smoke() {
	let fixture = test_project();
	let addr = free_loopback_address();
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

	wait_for_streamable_http(&addr, &mut child);

	let origin = format!("http://{addr}");
	let initialize = http_post(
		&addr,
		&[("Origin", origin.as_str()), ("Accept", "application/json")],
		r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
	);
	let session_id = initialize.header("mcp-session-id").expect("session id").to_owned();

	assert_eq!(initialize.status, "HTTP/1.1 200 OK");
	assert_eq!(initialize.json_body()["result"]["protocolVersion"], "2025-11-25");

	let tools_list = http_post(
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

	let above_profile = http_post(
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

	let observe_sse = http_post(
		&addr,
		&[
			("Origin", origin.as_str()),
			("Accept", "text/event-stream"),
			("Mcp-Session-Id", session_id.as_str()),
		],
		r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"},"name":"decodex_observe","arguments":{"limit":1}}}"#,
	);
	let observe_sse_body = observe_sse.body_text();

	assert_eq!(observe_sse.status, "HTTP/1.1 200 OK");
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

fn test_repo() -> TempDir {
	let repo = TempDir::new().expect("temp repo should exist");

	write_file(repo.path().join("Cargo.toml"), "[workspace]\n");
	write_file(repo.path().join("docs/index.md"), "# Docs\n");
	write_file(repo.path().join("docs/policy.md"), "# Policy\n");
	write_file(repo.path().join("docs/spec/runtime.md"), "# Runtime\n");

	repo
}

fn test_project() -> TestProject {
	let home = TempDir::new().expect("temp home should exist");
	let project = TempDir::new().expect("temp project should exist");
	let repo_path = project.path().join("repo");
	let project_config_dir = project.path().join("project");
	let config_path = project_config_dir.join("project.toml");

	fs::create_dir_all(repo_path.join(".worktrees")).expect("worktree root should exist");
	fs::create_dir_all(&project_config_dir).expect("project config dir should exist");

	write_file(repo_path.join("README.md"), "test repo\n");
	write_file(repo_path.join("docs/index.md"), "# Docs\n");
	write_file(
		project_config_dir.join("WORKFLOW.md"),
		r#"+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Read the repo policy first.
"#,
	);
	write_file(
		config_path.clone(),
		&format!(
			r#"service_id = "decodex"

[tracker]
api_key_env_var = "HOME"

[github]
token_env_var = "HOME"

[codex]
review = "standard"

[paths]
repo_root = "{}"
worktree_root = ".worktrees"
"#,
			repo_path.display()
		),
	);
	git_status_success(&repo_path, &["init", "-b", "main"]);
	git_status_success(&repo_path, &["config", "user.name", "Decodex Tests"]);
	git_status_success(&repo_path, &["config", "user.email", "decodex-tests@example.com"]);
	git_status_success(&repo_path, &["config", "commit.gpgsign", "false"]);
	git_status_success(&repo_path, &["add", "."]);
	git_status_success(&repo_path, &["commit", "-m", "bootstrap repo"]);

	TestProject {
		home_path: home.path().to_path_buf(),
		repo_path,
		config_path,
		_home: home,
		_project: project,
	}
}

fn git_status_success(cwd: &Path, args: &[&str]) {
	let output =
		hermetic_git_command().arg("-C").arg(cwd).args(args).output().expect("git should run");

	assert!(
		output.status.success(),
		"git {:?} failed: {}",
		args,
		String::from_utf8_lossy(&output.stderr)
	);
}

fn hermetic_git_command() -> Command {
	let mut command = Command::new("git");

	command
		.env("GIT_CONFIG_GLOBAL", "/dev/null")
		.env("GIT_CONFIG_SYSTEM", "/dev/null")
		.env("GIT_TERMINAL_PROMPT", "0")
		.env("GCM_INTERACTIVE", "never")
		.args([
			"-c",
			"core.hooksPath=/dev/null",
			"-c",
			"commit.gpgsign=false",
			"-c",
			"tag.gpgsign=false",
			"-c",
			"init.defaultBranch=main",
		]);

	command
}

fn write_file(path: PathBuf, contents: &str) {
	let parent = path.parent().expect("test path should have parent");

	fs::create_dir_all(parent).expect("parent directory should exist");
	fs::write(path, contents).expect("test file should write");
}

fn free_loopback_address() -> String {
	let listener = TcpListener::bind("127.0.0.1:0").expect("free loopback port should bind");
	let address = listener.local_addr().expect("loopback address should exist");

	address.to_string()
}

fn wait_for_streamable_http(addr: &str, child: &mut ChildGuard) {
	let deadline = Instant::now() + Duration::from_secs(10);

	loop {
		if let Some(status) = child.try_wait() {
			panic!("HTTP MCP process exited before accepting requests: {status:?}");
		}

		match http_options(addr) {
			Ok(response) if response.status == "HTTP/1.1 204 No Content" => return,
			Ok(response) => panic!("HTTP MCP readiness probe returned {}", response.status),
			Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
			Err(error) => panic!("HTTP MCP process did not listen at {addr}: {error}"),
		}
	}
}

fn http_options(addr: &str) -> Result<ParsedHttpResponse> {
	let request = format!(
		"OPTIONS /mcp HTTP/1.1\r\nHost: {addr}\r\nAccess-Control-Request-Method: POST\r\nContent-Length: 0\r\n\r\n"
	);
	let mut stream = TcpStream::connect(addr)?;
	let mut response = Vec::new();

	stream.write_all(request.as_bytes())?;
	stream.shutdown(Shutdown::Write)?;
	stream.read_to_end(&mut response)?;

	Ok(ParsedHttpResponse::parse(response))
}

fn http_post(addr: &str, headers: &[(&str, &str)], body: &str) -> ParsedHttpResponse {
	let mut request = format!(
		"POST /mcp HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
		body.len()
	);

	for (name, value) in headers {
		request.push_str(name);
		request.push_str(": ");
		request.push_str(value);
		request.push_str("\r\n");
	}

	request.push_str("\r\n");
	request.push_str(body);

	http_raw(addr, &request)
}

fn http_raw(addr: &str, request: &str) -> ParsedHttpResponse {
	let mut stream = TcpStream::connect(addr).expect("HTTP server should accept TCP");
	let mut response = Vec::new();

	stream.write_all(request.as_bytes()).expect("HTTP request should write");
	stream.shutdown(Shutdown::Write).expect("HTTP request should finish");
	stream.read_to_end(&mut response).expect("HTTP response should read");

	ParsedHttpResponse::parse(response)
}
