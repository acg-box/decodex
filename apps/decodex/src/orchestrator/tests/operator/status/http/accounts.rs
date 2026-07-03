use crate::orchestrator::tests::operator::status::http::{
	TempDir, TestEnvVarGuard, Value, fs, orchestrator,
};
#[test]
fn operator_state_endpoint_serves_account_api_snapshot() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be UTF-8"));
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			b"GET /api/accounts?refresh=1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
		)
		.expect("account response should build"),
	)
	.expect("account response should be utf-8");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(response.contains("Content-Type: application/json"));

	let body = response
		.split_once("\r\n\r\n")
		.map(|(_, body)| body)
		.expect("account response should include body");
	let data: Value = serde_json::from_str(body).expect("account response should be json");

	assert_eq!(data["accounts"], serde_json::json!([]));
	assert_eq!(data["usage_probe_error"], Value::Null);
	assert!(
		data["accounts_path"]
			.as_str()
			.is_some_and(|path| { path.ends_with(".codex/decodex/accounts.jsonl") })
	);
}

#[test]
fn operator_state_endpoint_persists_account_random_name_offset() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be UTF-8"));
	let accounts_dir = temp_dir.path().join(".codex/decodex");
	let accounts_path = accounts_dir.join("accounts.jsonl");

	fs::create_dir_all(&accounts_dir).expect("accounts dir should create");
	fs::write(
		&accounts_path,
		r#"{"email":"copy@example.com","tokens":{"access_token":"token","refresh_token":"refresh","account_id":"acct_123456"}}"#,
	)
	.expect("account pool should write");

	let body = br#"{"selector":"copy@example.com"}"#;
	let request = format!(
		"POST /api/accounts/reroll-name HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
		body.len(),
		String::from_utf8_lossy(body)
	);
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(request.as_bytes())
			.expect("account reroll response should build"),
	)
	.expect("account reroll response should be utf-8");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));

	let data: Value = serde_json::from_str(
		response
			.split_once("\r\n\r\n")
			.map(|(_, body)| body)
			.expect("account reroll response should include body"),
	)
	.expect("account reroll response should be json");

	assert_eq!(data["accounts"][0]["random_name_offset"], 1);
	assert_eq!(data["accounts"][0]["random_name_key"], "df65f796");
	assert_eq!(data["accounts"][0]["random_name"], "Logan");
	assert!(
		fs::read_to_string(accounts_dir.join("config.toml"))
			.expect("global config should read")
			.contains("df65f796 = 1")
	);
}

#[test]
fn operator_state_endpoint_rejects_removed_http_snapshot_routes() {
	for removed_path in ["/state", "/readyz"] {
		let response = String::from_utf8(
			orchestrator::build_operator_state_http_response(
				format!(
					"GET {removed_path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
				)
				.as_bytes(),
			)
			.expect("removed route response should build"),
		)
		.expect("removed route response should be utf-8");

		assert!(response.starts_with("HTTP/1.1 404 Not Found\r\n"));
		assert!(response.ends_with("not found"));
	}
}
