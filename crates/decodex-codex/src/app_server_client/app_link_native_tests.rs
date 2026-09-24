//! Installed hosted Apps policy uses a loopback backend and synthetic credentials only.
use super::*;
use crate::app_server_client::{InitializeCapabilities, RequestId, ServerEvent};
use tokio::io::{AsyncBufReadExt as _, BufReader};
struct Session {
	client: AppServerClient,
	child: tokio::process::Child,
	events: tokio::sync::mpsc::Receiver<ServerEvent>,
}
impl Session {
	async fn start(home: &Path) -> Self {
		let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
		let mut child = tokio::process::Command::new(binary)
			.arg("app-server")
			.env_clear()
			.env("HOME", home)
			.env("CODEX_HOME", home)
			.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
			.current_dir(home)
			.stdin(std::process::Stdio::piped())
			.stdout(std::process::Stdio::piped())
			.stderr(std::process::Stdio::null())
			.kill_on_drop(true)
			.spawn()
			.unwrap();
		let (client, events) =
			AppServerClient::from_io(child.stdout.take().unwrap(), child.stdin.take().unwrap());
		client.initialize(json!({"clientInfo":{"name":"isolated-app-links","version":"1"},"capabilities":InitializeCapabilities::for_chief()})).await.unwrap();
		Self { client, child, events }
	}

	async fn stop(&mut self) {
		self.child.kill().await.unwrap();
		self.child.wait().await.unwrap();
	}
}
#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY and host Python; isolated hosted Apps policy"]
async fn installed_link_policy_changes_preserve_pending_request_and_other_accounts() {
	tokio::time::timeout(Duration::from_secs(90), qualify())
		.await
		.expect("bounded hosted Apps fixture");
}
async fn qualify() {
	let home = tempfile::tempdir().unwrap();
	let root = home.path().canonicalize().unwrap();
	let cwd = root.to_str().unwrap();
	let audit = root.join("calls.jsonl");
	let mut backend = tokio::process::Command::new("python3")
		.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/app_link_backend.py"))
		.arg(&audit)
		.env_clear()
		.env("PATH", std::env::var_os("PATH").unwrap_or_default())
		.stdin(std::process::Stdio::null())
		.stdout(std::process::Stdio::piped())
		.stderr(std::process::Stdio::inherit())
		.kill_on_drop(true)
		.spawn()
		.unwrap();
	let mut lines = BufReader::new(backend.stdout.take().unwrap()).lines();
	let port: u16 = lines.next_line().await.unwrap().unwrap().parse().unwrap();
	let url = format!("http://127.0.0.1:{port}");
	std::fs::write(root.join("config.toml"),format!("model=\"gpt-5.6-sol\"\nchatgpt_base_url={url:?}\ncli_auth_credentials_store=\"file\"\nmodel_provider=\"fixture\"\n[features]\napps=true\ntool_call_mcp_elicitation=true\n[apps.calendar]\ndefault_tools_approval_mode=\"prompt\"\napprovals_reviewer=\"user\"\n[model_providers.fixture]\nname=\"Isolated fixture\"\nbase_url={url:?}\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).unwrap();
	let mut session = Session::start(&root).await;
	let installed =
		session.client.request("app/installed", json!({"forceRefresh":true})).await.unwrap();
	assert!(
		installed["apps"]
			.as_array()
			.unwrap()
			.iter()
			.any(|a| a["id"] == "calendar" && a["callable"] == true)
	);
	let start=session.client.thread_start(json!({"cwd":root,"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":"read-only"})).await.unwrap();
	let thread = start["thread"]["id"].as_str().unwrap().to_owned();
	start_turn(&session.client, &thread).await;
	let (id, params) = approval(&mut session.events, &thread, "work").await;
	let guard =
		session.client.server_request_guard(&id, "mcpServer/elicitation/request", &params).unwrap();
	let read = session.client.app_link_settings(cwd, "calendar", "work").await.unwrap();
	assert_eq!(read.user_mode, None);
	let saved = session
		.client
		.write_app_link_setting_guarded(
			&read,
			AppLinkSettingEdit::ApprovalMode(Some("approve".into())),
			guard.clone(),
		)
		.await
		.unwrap();
	assert!(!saved.overridden);
	assert!(guard.is_live(), "config reload must not resolve the waiting approval");
	session
		.client
		.respond(id, json!({"action":"accept","content":null,"_meta":null}))
		.await
		.unwrap();
	finish(&mut session.events, &thread).await;
	assert!(!guard.is_live());
	assert!(
		session
			.client
			.write_app_link_setting_guarded(&read, AppLinkSettingEdit::ApprovalMode(None), guard)
			.await
			.is_err()
	);
	start_turn(&session.client, &thread).await;
	finish(&mut session.events, &thread).await;
	assert_eq!(
		session.client.app_link_settings(cwd, "calendar", "personal").await.unwrap().user_mode,
		None
	);
	start_turn(&session.client, &thread).await;
	let (id, _) = approval(&mut session.events, &thread, "personal").await;
	session.client.respond(id, json!({"action":"accept","content":null})).await.unwrap();
	finish(&mut session.events, &thread).await;
	let current = session.client.app_link_settings(cwd, "calendar", "work").await.unwrap();
	session
		.client
		.write_app_link_setting(&current, AppLinkSettingEdit::ApprovalMode(None))
		.await
		.unwrap();
	start_turn(&session.client, &thread).await;
	let (id, _) = approval(&mut session.events, &thread, "work").await;
	session.client.respond(id, json!({"action":"accept","content":null})).await.unwrap();
	finish(&mut session.events, &thread).await;
	session.stop().await;
	let mut session = Session::start(&root).await;
	session.client.thread_resume(json!({"threadId":thread})).await.unwrap();
	assert_eq!(
		session.client.app_link_settings(cwd, "calendar", "work").await.unwrap().user_mode,
		None
	);
	start_turn(&session.client, &thread).await;
	let (id, _) = approval(&mut session.events, &thread, "work").await;
	session.client.respond(id, json!({"action":"accept","content":null})).await.unwrap();
	finish(&mut session.events, &thread).await;
	qualify_reviewer(&mut session, cwd, &thread).await;
	session.stop().await;
	backend.kill().await.unwrap();
	backend.wait().await.unwrap();
	let calls = std::fs::read_to_string(audit).unwrap();
	let records: Vec<Value> = calls.lines().map(|s| serde_json::from_str(s).unwrap()).collect();
	let links: Vec<&str> = records.iter().filter_map(|v| v["link_id"].as_str()).collect();
	assert_eq!(links, ["work", "work", "personal", "work", "work", "work", "personal"]);
	assert_eq!(
		records.iter().filter(|v| v["guardian"] == true).count(),
		1,
		"only work link uses native Guardian"
	);
}
async fn qualify_reviewer(session: &mut Session, cwd: &str, thread: &str) {
	let current = session.client.app_link_settings(cwd, "calendar", "work").await.unwrap();
	session
		.client
		.write_app_link_setting(&current, AppLinkSettingEdit::Reviewer(Some("auto_review".into())))
		.await
		.unwrap();
	start_turn(&session.client, thread).await;
	finish(&mut session.events, thread).await;
	assert_eq!(
		session.client.app_link_settings(cwd, "calendar", "personal").await.unwrap().user_reviewer,
		None
	);
	start_turn(&session.client, thread).await;
	let (id, _) = approval(&mut session.events, thread, "personal").await;
	session.client.respond(id, json!({"action":"accept","content":null})).await.unwrap();
	finish(&mut session.events, thread).await;
}
async fn start_turn(client: &AppServerClient, thread: &str) {
	client.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Use [$calendar](app://calendar) to create an isolated fixture event."}]})).await.unwrap();
}
async fn approval(
	events: &mut tokio::sync::mpsc::Receiver<ServerEvent>,
	thread: &str,
	link: &str,
) -> (RequestId, Value) {
	loop {
		match events.recv().await.expect("approval event") {
			ServerEvent::Request { id, method, params } => {
				assert_eq!(method, "mcpServer/elicitation/request");
				assert_eq!(params["threadId"], thread);
				assert_eq!(params["serverName"], "codex_apps");
				assert_eq!(params["_meta"]["connector_id"], "calendar");
				assert_eq!(params["_meta"]["link_id"], link);
				return (id, params);
			},
			ServerEvent::Notification { method, .. } if method == "turn/completed" =>
				panic!("expected account-specific approval"),
			ServerEvent::Closed(_) => panic!("native closed"),
			_ => {},
		}
	}
}
async fn finish(events: &mut tokio::sync::mpsc::Receiver<ServerEvent>, thread: &str) {
	loop {
		match events.recv().await.expect("turn event") {
			ServerEvent::Notification { method, params } if method == "turn/completed" => {
				assert_eq!(params["threadId"], thread);
				assert_eq!(params["turn"]["status"], "completed");
				return;
			},
			ServerEvent::Request { .. } => panic!("unexpected additional approval"),
			ServerEvent::Closed(_) => panic!("native closed"),
			_ => {},
		}
	}
}
