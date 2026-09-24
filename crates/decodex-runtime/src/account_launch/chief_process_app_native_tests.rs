//! Installed hosted Apps policy uses a loopback backend and synthetic credentials only.
use super::*;
use decodex_codex::app_server_client::{
	AppLinkSettingEdit, InitializeCapabilities, RequestId, ServerEvent,
};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
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
			.expect("native fixture");
		let (client, events) = AppServerClient::from_io(
			child.stdout.take().expect("native fixture"),
			child.stdin.take().expect("native fixture"),
		);
		client.initialize(json!({"clientInfo":{"name":"isolated-app-links","version":"1"},"capabilities":InitializeCapabilities::for_chief()})).await.expect("native fixture");
		Self { client, child, events }
	}

	async fn stop(&mut self) {
		self.child.kill().await.expect("native fixture");
		self.child.wait().await.expect("native fixture");
	}
}
#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY and host Python; isolated hosted Apps policy"]
async fn installed_app_service_preserves_requests_receipts_and_native_link_policy() {
	tokio::time::timeout(Duration::from_secs(90), qualify())
		.await
		.expect("bounded hosted Apps fixture");
}
async fn start_backend(root: &Path, audit: &Path) -> tokio::process::Child {
	let mut backend = tokio::process::Command::new("python3")
		.arg(
			Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("../decodex-codex/tests/fixtures/app_link_backend.py"),
		)
		.arg(audit)
		.env_clear()
		.env("PATH", std::env::var_os("PATH").unwrap_or_default())
		.stdin(std::process::Stdio::null())
		.stdout(std::process::Stdio::piped())
		.stderr(std::process::Stdio::inherit())
		.kill_on_drop(true)
		.spawn()
		.expect("native fixture");
	let mut lines = BufReader::new(backend.stdout.take().expect("native fixture")).lines();
	let port: u16 = lines
		.next_line()
		.await
		.expect("native fixture")
		.expect("native fixture")
		.parse()
		.expect("native fixture");
	let url = format!("http://127.0.0.1:{port}");
	std::fs::write(root.join("config.toml"),format!("model=\"gpt-5.6-sol\"\nchatgpt_base_url={url:?}\ncli_auth_credentials_store=\"file\"\nmodel_provider=\"fixture\"\n[features]\napps=true\ntool_call_mcp_elicitation=true\n[apps.calendar]\ndefault_tools_approval_mode=\"prompt\"\napprovals_reviewer=\"user\"\n[model_providers.fixture]\nname=\"Isolated fixture\"\nbase_url={url:?}\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("native fixture");
	backend
}
async fn qualify() {
	let home = tempfile::tempdir().expect("native fixture");
	let root = home.path().canonicalize().expect("native fixture");
	let cwd = root.to_str().expect("native fixture");
	let audit = root.join("calls.jsonl");
	let mut backend = start_backend(&root, &audit).await;
	let mut session = Session::start(&root).await;
	let installed = session
		.client
		.request("app/installed", json!({"forceRefresh":true}))
		.await
		.expect("native fixture");
	assert!(
		installed["apps"]
			.as_array()
			.expect("native fixture")
			.iter()
			.any(|a| a["id"] == "calendar" && a["callable"] == true)
	);
	let start=session.client.thread_start(json!({"cwd":root,"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":"read-only"})).await.expect("native fixture");
	let thread = start["thread"]["id"].as_str().expect("native fixture").to_owned();
	start_turn(&session.client, &thread).await;
	let (id, params) = approval(&mut session.events, &thread, "work").await;
	let guard = session
		.client
		.server_request_guard(&id, "mcpServer/elicitation/request", &params)
		.expect("native fixture");
	service_edits(&root, &session.client, &thread, &id, &params).await;
	let read =
		session.client.app_link_settings(cwd, "calendar", "work").await.expect("native fixture");
	assert!(guard.is_live(), "config reload must not resolve the waiting approval");
	session
		.client
		.respond(id, json!({"action":"accept","content":null,"_meta":null}))
		.await
		.expect("native fixture");
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
		session
			.client
			.app_link_settings(cwd, "calendar", "personal")
			.await
			.expect("native fixture")
			.user_mode,
		None
	);
	start_turn(&session.client, &thread).await;
	let (id, _) = approval(&mut session.events, &thread, "personal").await;
	session
		.client
		.respond(id, json!({"action":"accept","content":null}))
		.await
		.expect("native fixture");
	finish(&mut session.events, &thread).await;
	let current =
		session.client.app_link_settings(cwd, "calendar", "work").await.expect("native fixture");
	session
		.client
		.write_app_link_setting(&current, AppLinkSettingEdit::ApprovalMode(None))
		.await
		.expect("native fixture");
	start_turn(&session.client, &thread).await;
	let (id, _) = approval(&mut session.events, &thread, "work").await;
	session
		.client
		.respond(id, json!({"action":"accept","content":null}))
		.await
		.expect("native fixture");
	finish(&mut session.events, &thread).await;
	session.stop().await;
	let mut session = Session::start(&root).await;
	session.client.thread_resume(json!({"threadId":thread})).await.expect("native fixture");
	assert_eq!(
		session
			.client
			.app_link_settings(cwd, "calendar", "work")
			.await
			.expect("native fixture")
			.user_mode,
		None
	);
	start_turn(&session.client, &thread).await;
	let (id, _) = approval(&mut session.events, &thread, "work").await;
	session
		.client
		.respond(id, json!({"action":"accept","content":null}))
		.await
		.expect("native fixture");
	finish(&mut session.events, &thread).await;
	qualify_reviewer(&mut session, cwd, &thread).await;
	session.stop().await;
	backend.kill().await.expect("native fixture");
	backend.wait().await.expect("native fixture");
	let calls = std::fs::read_to_string(audit).expect("native fixture");
	let records: Vec<Value> =
		calls.lines().map(|s| serde_json::from_str(s).expect("native fixture")).collect();
	let links: Vec<&str> = records.iter().filter_map(|v| v["link_id"].as_str()).collect();
	assert_eq!(links, ["work", "work", "personal", "work", "work", "work", "personal"]);
	assert_eq!(
		records.iter().filter(|v| v["guardian"] == true).count(),
		1,
		"only work link uses native Guardian"
	);
}
async fn qualify_reviewer(session: &mut Session, cwd: &str, thread: &str) {
	let current =
		session.client.app_link_settings(cwd, "calendar", "work").await.expect("native fixture");
	session
		.client
		.write_app_link_setting(&current, AppLinkSettingEdit::Reviewer(Some("auto_review".into())))
		.await
		.expect("native fixture");
	start_turn(&session.client, thread).await;
	finish(&mut session.events, thread).await;
	assert_eq!(
		session
			.client
			.app_link_settings(cwd, "calendar", "personal")
			.await
			.expect("native fixture")
			.user_reviewer,
		None
	);
	start_turn(&session.client, thread).await;
	let (id, _) = approval(&mut session.events, thread, "personal").await;
	session
		.client
		.respond(id, json!({"action":"accept","content":null}))
		.await
		.expect("native fixture");
	finish(&mut session.events, thread).await;
}
async fn start_turn(client: &AppServerClient, thread: &str) {
	client.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Use [$calendar](app://calendar) to create an isolated fixture event."}]})).await.expect("native fixture");
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

async fn service_edits(
	root: &Path,
	client: &AppServerClient,
	thread: &str,
	id: &RequestId,
	params: &Value,
) {
	use decodex_protocol::{
		ChiefAppApprovalMode as Mode, ChiefAppReviewer as Reviewer, ChiefAppSettingEdit as Edit,
		ChiefAppSettingsResult as State,
	};
	let owned =
		OwnedReviewer::new(root, client, thread, params["turnId"].as_str().expect("turn")).await;
	let event = owned
		.store
		.enqueue_chief_event(decodex_database::EnqueueChiefEvent {
			source_event_id: "native-app-approval".into(),
			work_item_id: "root".into(),
			event_kind: "server_request_pending".into(),
			payload: json!({"id":id,"method":"mcpServer/elicitation/request","params":params})
				.to_string(),
		})
		.await
		.expect("event");
	for (n, edit) in [
		Edit::ApprovalMode(Some(Mode::Approve)),
		Edit::ApprovalMode(None),
		Edit::Reviewer(Some(Reviewer::AutoReview)),
		Edit::Reviewer(None),
		Edit::ApprovalMode(Some(Mode::Approve)),
	]
	.iter()
	.enumerate()
	{
		let source = || async { Some(owned.source(&owned.key)) };
		let State::Available { can_update: true, review_token, .. } =
			crate::chief_app_settings::read(&owned.store, source, event.id).await
		else {
			panic!("native service review")
		};
		let attempt = format!("native-app-{n}");
		crate::chief_app_settings::write(
			&owned.store,
			source,
			crate::chief_app_settings::Selection {
				event: event.id,
				review: &review_token,
				edit,
				attempt_id: &attempt,
			},
		)
		.await
		.expect("service save");
		assert!(client.server_request_guard(id, "mcpServer/elicitation/request", params).is_some());
		assert!(
			owned.store.get_chief_inbox_event(event.id).await.expect("event").disposition.is_none()
		);
		let State::Available { last_edit: Some(receipt), user_mode, user_reviewer, .. } =
			crate::chief_app_settings::read(&owned.store, source, event.id).await
		else {
			panic!("saved receipt")
		};
		assert_eq!(receipt.outcome, "saved");
		assert!(receipt.saved_version.is_some());
		let (field, value) = edit.native_value();
		assert_eq!(
			if field == "approvals_reviewer" {
				user_reviewer.as_deref()
			} else {
				user_mode.as_deref()
			},
			value
		);
		assert!(
			crate::chief_app_settings::write(
				&owned.store,
				source,
				crate::chief_app_settings::Selection {
					event: event.id,
					review: &review_token,
					edit,
					attempt_id: "replay"
				}
			)
			.await
			.is_err()
		);
	}
	let reopened = SqliteStore::open(&owned.root.paths()).expect("reopen");
	let native = client
		.app_link_settings(root.to_str().expect("path"), "calendar", "work")
		.await
		.expect("native config");
	let receipt = reopened
		.chief_app_settings_receipt(crate::chief_config_settings::digest(native.config_file()))
		.await
		.expect("receipt")
		.expect("saved");
	assert_eq!(receipt.state, "saved");
	assert_eq!(receipt.saved_version.as_deref(), Some(native.config_version()));
}
