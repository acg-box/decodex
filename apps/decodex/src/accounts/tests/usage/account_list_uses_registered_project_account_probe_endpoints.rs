use std::{
	fs,
	io::{Read as _, Write as _},
	net::TcpListener,
	thread,
};

use tempfile::TempDir;

use crate::{accounts, runtime, test_support::TestEnvVarGuard};

#[test]
fn account_list_uses_registered_project_account_probe_endpoints() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let decodex_dir = temp_dir.path().join(".codex/decodex");
	let accounts_path = decodex_dir.join("accounts.jsonl");
	let project_dir = temp_dir.path().join("projects/mock");
	let (usage_endpoint, reset_credits_endpoint) = start_account_usage_fixture_with_reset_credits();

	fs::create_dir_all(&decodex_dir).expect("decodex dir should create");
	fs::create_dir_all(&project_dir).expect("project dir should create");
	fs::write(
		&accounts_path,
		r#"{"email":"mock@example.test","auth_mode":"chatgpt","tokens":{"access_token":"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig","refresh_token":"refresh-secret","account_id":"acct_mock"}}"#,
	)
	.expect("accounts should write");
	fs::write(project_dir.join("WORKFLOW.md"), "# Mock\n").expect("workflow should write");
	fs::write(
		project_dir.join("project.toml"),
		format!(
			r#"
service_id = "mock"

[tracker]
api_key_env_var = "MOCK_LINEAR_API_KEY"

[github]
token_env_var = "MOCK_GITHUB_TOKEN"

[codex]
review = "strict"

[codex.accounts]
usage_endpoint = "{usage_endpoint}"
reset_credits_endpoint = "{reset_credits_endpoint}"

[paths]
repo_root = "{}"
worktree_root = ".worktrees"
"#,
			temp_dir.path().display()
		),
	)
	.expect("project config should write");

	let state_store = runtime::open_runtime_store().expect("state store should open");

	runtime::register_project_config(&state_store, &project_dir, true)
		.expect("project should register");

	let response =
		accounts::account_list_with_cached_usage(true).expect("account list should hydrate");

	assert_eq!(response.accounts[0].status, "available");
	assert_eq!(response.accounts[0].plan_type.as_deref(), Some("pro"));
	assert_eq!(response.accounts[0].primary_remaining_percent, Some(87));
	assert_eq!(response.accounts[0].secondary_remaining_percent, Some(65));
	assert_eq!(response.accounts[0].reset_credits_available_count, Some(3));
	assert_eq!(response.accounts[0].reset_credits.len(), 3);
	assert_eq!(response.usage_probe_error, None);
	assert!(
		!fs::read_to_string(&accounts_path)
			.expect("accounts should read")
			.contains("auth_failed_at_unix_epoch")
	);
}

fn start_account_usage_fixture_with_reset_credits() -> (String, String) {
	let listener = TcpListener::bind("127.0.0.1:0").expect("usage fixture should bind");
	let address = listener.local_addr().expect("usage fixture address should resolve");

	thread::spawn(move || {
		for _ in 0..2 {
			let (mut stream, _peer) =
				listener.accept().expect("usage fixture request should arrive");
			let mut request = [0_u8; 4_096];
			let bytes_read = stream.read(&mut request).expect("request should read");
			let request = String::from_utf8_lossy(&request[..bytes_read]);
			let body = if request.starts_with("GET /wham/usage ") {
				r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":13,"limit_window_seconds":18000,"reset_at":1800018000},"secondary_window":{"used_percent":35,"limit_window_seconds":604800,"reset_at":1800604800}},"credits":{"has_credits":true,"unlimited":false,"balance":"0.00"}}"#
			} else {
				r#"{"available_count":3,"total_earned_count":5,"credits":[{"expires_at":"2026-07-18T08:36:00Z","status":"available"},{"expires_at":"2026-07-25T15:15:00Z","status":"available"},{"expires_at":"2026-08-01T04:06:00Z","status":"available"}]}"#
			};
			let response = format!(
				"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
				body.len(),
				body
			);

			stream.write_all(response.as_bytes()).expect("response should write");
		}
	});

	(format!("http://{address}/wham/usage"), format!("http://{address}/wham/reset-credits"))
}
