//! Exercise native notifications through the retained bridge with synthetic credentials.
use super::{NativeSession, json};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use decodex_codex::app_server_client::{AccountNudgeCreditType, AccountNudgeOutcome};
use std::{io::Write, os::unix::fs::OpenOptionsExt, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; production attestation and synthetic vault projection"]
async fn installed_native_account_nudge_uses_attested_control_and_ephemeral_auth() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().expect("isolated native control home");
	let directory = home.path().canonicalize().expect("canonical native home");
	let codex_home = directory.join(".codex");
	std::fs::create_dir(&codex_home).expect("isolated Codex home");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("loopback backend");
	let address = listener.local_addr().expect("backend address");
	std::fs::write(
		codex_home.join("config.toml"),
		format!("chatgpt_base_url = \"http://{address}\"\ncli_auth_credentials_store = \"file\"\n"),
	)
	.expect("isolated native configuration");
	let owner = tokio::task::spawn_blocking(move || {
		let mut child =
			crate::account_launch::process::native_control_tests::initialized_control_child(
				&binary, &directory,
			);
		let account =
			crate::account_launch::process::native_control_tests::read_native_account(&mut child);
		assert_eq!(
			account["workspaceRouting"],
			json!({
				"chatgptAccountId": "workspace-fixture",
				"backendOrigin": format!("https://{address}"),
				"accountRoutingOverride": "NO_CONSTRAINT"
			})
		);
		child
	});
	let control = async {
		let mut child = owner.await.expect("control owner");
		let (client, _events) =
			child.retain_account_control_connection().expect("initialized control bridge");
		let actual = client.send_account_nudge(AccountNudgeCreditType::Credits).await;
		client.close();
		tokio::task::spawn_blocking(move || child.shutdown())
			.await
			.expect("shutdown owner")
			.expect("native child cleanup");
		actual
	};
	let (actual, ()) = tokio::time::timeout(Duration::from_secs(60), async {
		tokio::join!(
			control,
			serve_notification(&listener, "200 OK", AccountNudgeCreditType::Credits)
		)
	})
	.await
	.expect("bounded attested notification");
	assert_eq!(actual, AccountNudgeOutcome::Sent);
	assert!(
		!codex_home.join("auth.json").exists(),
		"ephemeral projection must not persist credentials"
	);
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native account notification qualification"]
async fn installed_native_account_nudge_crosses_bridge_without_retry() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().expect("isolated native home");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("loopback backend");
	let address = listener.local_addr().expect("backend address");
	write_fixture_credentials(home.path());
	std::fs::write(
		home.path().join("config.toml"),
		format!("chatgpt_base_url = \"http://{address}\"\ncli_auth_credentials_store = \"file\"\n"),
	)
	.expect("isolated backend configuration");
	let session = NativeSession::start(&binary, home.path());
	for (status, purpose, expected) in [
		("200 OK", AccountNudgeCreditType::Credits, AccountNudgeOutcome::Sent),
		(
			"429 Too Many Requests",
			AccountNudgeCreditType::UsageLimit,
			AccountNudgeOutcome::CooldownActive,
		),
		(
			"500 Internal Server Error",
			AccountNudgeCreditType::Credits,
			AccountNudgeOutcome::Uncertain,
		),
	] {
		let (actual, ()) = tokio::time::timeout(Duration::from_secs(20), async {
			tokio::join!(
				session.client.send_account_nudge(purpose),
				serve_notification(&listener, status, purpose)
			)
		})
		.await
		.expect("bounded bridge notification");
		assert_eq!(actual, expected);
	}
	// Keep the bridge alive while checking that no fourth request follows the failure.
	assert!(
		tokio::time::timeout(
			Duration::from_millis(300),
			serve_notification(&listener, "200 OK", AccountNudgeCreditType::Credits)
		)
		.await
		.is_err()
	);
	drop(session);
}

fn write_fixture_credentials(home: &std::path::Path) {
	let claims = json!({"email":"fixture@example.test","exp":4102444800_u64,
		"https://api.openai.com/auth":{"chatgpt_account_id":"workspace-fixture",
		"chatgpt_user_id":"user-fixture","chatgpt_plan_type":"team"}});
	let token = format!(
		"{}.{}.fixture-signature",
		URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
		URL_SAFE_NO_PAD.encode(claims.to_string())
	);
	let auth = json!({"auth_mode":"chatgpt","tokens":{"id_token":token,
		"access_token":"fixture-only","refresh_token":"fixture-only","account_id":"workspace-fixture"},
		"last_refresh":"2026-09-21T15:00:00Z"});
	let mut file = std::fs::OpenOptions::new()
		.write(true)
		.create_new(true)
		.mode(0o600)
		.open(home.join("auth.json"))
		.expect("private fixture credentials");
	file.write_all(auth.to_string().as_bytes()).expect("synthetic credentials");
}

pub(crate) async fn serve_notification(
	listener: &tokio::net::TcpListener,
	status: &str,
	purpose: AccountNudgeCreditType,
) {
	serve_notification_with_gate(listener, status, purpose, None).await;
}

pub(crate) async fn serve_notification_with_gate(
	listener: &tokio::net::TcpListener,
	status: &str,
	purpose: AccountNudgeCreditType,
	gate: Option<std::sync::Arc<(tokio::sync::Notify, tokio::sync::Notify)>>,
) {
	let mut connections = tokio::task::JoinSet::new();
	loop {
		tokio::select! {
			accepted = listener.accept() => {
				let (stream, _) = accepted.expect("native fixture connection");
				connections.spawn(handle_request(stream, status.to_owned(), purpose, gate.clone()));
			},
			finished = connections.join_next(), if !connections.is_empty() => {
				if finished.expect("request task").expect("valid native request") { return; }
			},
		}
	}
}

async fn handle_request(
	stream: tokio::net::TcpStream,
	status: String,
	purpose: AccountNudgeCreditType,
	gate: Option<std::sync::Arc<(tokio::sync::Notify, tokio::sync::Notify)>>,
) -> bool {
	let mut stream = BufReader::new(stream);
	let mut line = String::new();
	stream.read_line(&mut line).await.expect("request line");
	let request_line = line.clone();
	let mut length = None;
	let mut account = None;
	loop {
		line.clear();
		assert!(stream.read_line(&mut line).await.expect("request header") > 0);
		if line == "\r\n" {
			break;
		}
		let (name, value) = line.split_once(':').expect("HTTP header");
		match name.to_ascii_lowercase().as_str() {
			"content-length" => length = Some(value.trim().parse::<usize>().expect("body length")),
			"chatgpt-account-id" => account = Some(value.trim().to_owned()),
			"x-openai-codex-luna-reserve" => panic!("notification must not use Reserve"),
			_ => {},
		}
	}
	if request_line == "GET /api/codex/accounts/check HTTP/1.1\r\n" {
		if let Some(gate) = gate {
			gate.0.notify_one();
			gate.1.notified().await;
		}
		assert_eq!(account.as_deref(), Some("workspace-fixture"));
		let origin = format!("https://{}", stream.get_ref().local_addr().expect("loopback origin"));
		let body = json!({"default_account_id":"other-workspace", "accounts":[
			{"id":"other-workspace", "workspace_backend_origin":"https://other-workspace.invalid",
			"account_routing_override":"us_cr"},
			{"id":"workspace-fixture", "workspace_backend_origin":origin,
			"account_routing_override":"NO_CONSTRAINT"}]})
		.to_string();
		stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.expect("workspace routing fixture");
		return false;
	}
	if request_line.starts_with("GET ") {
		stream
			.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
			.await
			.expect("unrelated native discovery response");
		return false;
	}
	assert_eq!(request_line, "POST /api/codex/accounts/send_add_credits_nudge_email HTTP/1.1\r\n");
	assert_eq!(account.as_deref(), Some("workspace-fixture"));
	let length = length.expect("bounded request body");
	assert!(length < 1024);
	let mut body = vec![0; length];
	stream.read_exact(&mut body).await.expect("notification body");
	assert_eq!(
		serde_json::from_slice::<serde_json::Value>(&body).expect("JSON body"),
		json!({"credit_type":purpose})
	);
	stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}").as_bytes())
		.await.expect("native notification response");
	true
}
