//! Opt-in installed-native qualification with a local Responses fixture and no credentials.
use super::*;
#[path = "chief_process_native_context_tests.rs"] mod context;
#[path = "chief_process_native_detail_tests.rs"] mod detail;
#[path = "chief_process_native_misalignment_tests.rs"] mod misalignment;
#[path = "chief_process_native_realtime_tests.rs"] mod realtime;
#[path = "chief_process_native_usage_tests.rs"] mod usage;
use serde_json::json;
use std::{
	io::{BufRead, BufReader},
	process::{Child, Command, Stdio},
	sync::mpsc as sync_mpsc,
};

const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";

struct NativeChild(Child);

impl Drop for NativeChild {
	fn drop(&mut self) {
		let _ = self.0.kill();
		let _ = self.0.wait();
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native settings through retained bridge"]
async fn installed_native_account_settings_cross_retained_bridge_and_cold_restart() {
	use decodex_codex::app_server_client::AppLinkSettingEdit;
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().unwrap();
	assert!(!home.path().join("config.toml").exists());
	let cwd = home.path().to_str().unwrap();
	let session = NativeSession::start(&binary, home.path());
	let saved = tokio::time::timeout(Duration::from_secs(20), async {
		let before =
			session.client.app_link_settings(cwd, "calendar.app", "工作.\"link\\1").await.unwrap();
		let saved = session
			.client
			.write_app_link_setting(
				&before,
				AppLinkSettingEdit::ApprovalMode(Some("prompt".into())),
			)
			.await
			.unwrap();
		assert_eq!(saved.settings.effective_mode.as_deref(), Some("prompt"));
		assert!(matches!(
			session
				.client
				.write_app_link_setting(
					&before,
					AppLinkSettingEdit::Reviewer(Some("auto_review".into()))
				)
				.await,
			Err(ClientError::Remote(_))
		));
		saved.settings
	})
	.await
	.unwrap();
	assert!(home.path().join("config.toml").is_file());
	drop(session);
	let reopened = NativeSession::start(&binary, home.path());
	tokio::time::timeout(Duration::from_secs(20), async {
		assert!(matches!(
			reopened
				.client
				.write_app_link_setting(&saved, AppLinkSettingEdit::Reviewer(None))
				.await,
			Err(ClientError::InvalidFrame)
		));
		let cold =
			reopened.client.app_link_settings(cwd, "calendar.app", "工作.\"link\\1").await.unwrap();
		assert_eq!(cold.user_mode.as_deref(), Some("prompt"));
		let cleared = reopened
			.client
			.write_app_link_setting(&cold, AppLinkSettingEdit::ApprovalMode(None))
			.await
			.unwrap();
		assert_eq!(cleared.settings.user_mode, None);
	})
	.await
	.unwrap();
}

#[tokio::test]
#[ignore = "requires explicit DECODEX_TEST_CODEX_BINARY; isolated native history qualification"]
async fn installed_native_history_reads_cross_retained_bridge_without_new_model_work() {
	let unfiltered = qualify_notification_media(false).await;
	let filtered = qualify_notification_media(true).await;
	assert_eq!(filtered, unfiltered, "notification filtering changed model images");
}

async fn qualify_notification_media(omit_media: bool) -> Vec<Vec<Value>> {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().expect("native notification media fixture");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
		.await
		.expect("native notification media fixture");
	let address = listener.local_addr().expect("native notification media fixture");
	let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
	let backend =
		tokio::spawn(serve_with_bodies(listener, Arc::clone(&requests), Some(Arc::clone(&bodies))));
	std::fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[model_providers.fixture]\nname = \"Isolated history fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n[features]\nomit_app_server_notification_media = {omit_media}\n")).expect("native notification media fixture");
	let mut session = NativeSession::start(&binary, home.path());
	let (id, before) = tokio::time::timeout(Duration::from_secs(30), async {
		let client = &session.client;
		let started = client.thread_start(json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only"})).await.expect("native notification media fixture");
		let id = started["thread"]["id"].as_str().expect("native notification media fixture");
		assert!(matches!(client.thread_timeline_page(id, None, 30).await,
			Err(ClientError::Remote(error)) if error.code == -32601));
		let read = client.thread_read(json!({"threadId":id,"includeTurns":false})).await.expect("native notification media fixture");
		assert_eq!(read["thread"]["id"], id);
		assert_eq!(read["thread"]["historyMode"], "paginated");
		qualify_history(client, &mut session.events, id, &requests, omit_media).await;
		(id.to_owned(), read_history(client, id, &requests).await)
	}).await.expect("native notification media fixture");
	drop(session);
	let reopened = NativeSession::start(&binary, home.path());
	let after = tokio::time::timeout(
		Duration::from_secs(30),
		read_history(&reopened.client, &id, &requests),
	)
	.await
	.expect("native notification media fixture");
	assert_eq!(
		serde_json::to_value(before).expect("native notification media fixture"),
		serde_json::to_value(after).expect("native notification media fixture"),
		"restart changed native history"
	);
	let model_images = {
		let bodies = bodies.lock().expect("native notification media fixture");
		assert_eq!(bodies.len(), 2);
		bodies
			.iter()
			.enumerate()
			.map(|(turn_index, body)| {
				let images = body["input"]
					.as_array()
					.expect("native notification media fixture")
					.iter()
					.filter(|item| item["role"] == "user")
					.filter_map(|item| item["content"].as_array())
					.flatten()
					.filter(|part| part["type"] == "input_image")
					.cloned()
					.collect::<Vec<_>>();
				assert_eq!(images.len(), turn_index + 1, "native model request lost user image");
				images
			})
			.collect()
	};
	drop(reopened);
	backend.abort();
	model_images
}

struct NativeSession {
	child: NativeChild,
	reader: Option<JoinHandle<()>>,
	bridge: ChiefProcessBridge,
	client: AppServerClient,
	events: mpsc::Receiver<ServerEvent>,
}

impl Drop for NativeSession {
	fn drop(&mut self) {
		self.bridge.close();
		let _ = self.child.0.kill();
		let _ = self.child.0.wait();
		if let Some(reader) = self.reader.take() {
			let _ = reader.join();
		}
	}
}

impl NativeSession {
	fn start(binary: &std::ffi::OsStr, home: &std::path::Path) -> Self {
		let mut child = NativeChild(
			Command::new(binary)
				.arg("app-server")
				.env_clear()
				.env("HOME", home)
				.env("CODEX_HOME", home)
				.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
				.current_dir(home)
				.stdin(Stdio::piped())
				.stdout(Stdio::piped())
				.stderr(Stdio::null())
				.spawn()
				.expect("native session setup"),
		);
		let mut stdin = child.0.stdin.take().expect("native session setup");
		let stdout = child.0.stdout.take().expect("native session setup");
		let (send, receive) = sync_mpsc::sync_channel(64);
		let reader = thread::spawn(move || {
			for line in BufReader::new(stdout).lines() {
				let Ok(line) = line else { break };
				if send.send(InboundFrame::fixture(line.as_bytes())).is_err() {
					break;
				}
			}
		});
		writeln!(stdin, "{}", json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"decodex_native_history_test","version":"0.1"},"capabilities":decodex_codex::app_server_client::InitializeCapabilities::default()}})).expect("native session setup");
		let deadline = std::time::Instant::now() + Duration::from_secs(15);
		loop {
			let frame = receive
				.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
				.expect("native session setup")
				.into_contiguous();
			let value: Value = serde_json::from_slice(&frame).expect("native session setup");
			if value["id"] == 1 {
				assert!(value.get("result").is_some(), "native initialization failed");
				break;
			}
		}
		writeln!(stdin, "{}", json!({"method":"initialized"})).expect("native session setup");
		let binding = AccountBinding::fixture(
			decodex_core::AccountId::new("10000000-0000-4000-8000-000000000001")
				.expect("native session setup"),
			home.into(),
		);
		let (bridge, client, events) = ChiefProcessBridge::start(
			Box::new(stdin),
			receive,
			binding,
			Arc::new(AtomicBool::new(false)),
			2,
			vec![],
		)
		.expect("native session setup");
		Self { child, reader: Some(reader), bridge, client, events }
	}
}

async fn qualify_history(
	client: &AppServerClient,
	events: &mut mpsc::Receiver<ServerEvent>,
	thread_id: &str,
	requests: &std::sync::atomic::AtomicUsize,
	omit_media: bool,
) {
	for text in ["First native history input", "Second native history input"] {
		let turn = client
			.turn_start(json!({"threadId":thread_id,"input":[{"type":"text","text":text},{"type":"image","url":format!("data:image/png;base64,{PNG}")}]}))
			.await
			.expect("native history fixture operation");
		let mut user_events = HashSet::new();
		loop {
			let event = events.recv().await.expect("native event stream");
			if let ServerEvent::Notification { method, params } = &event
				&& matches!(method.as_str(), "item/started" | "item/completed")
				&& params["threadId"] == thread_id
				&& params["turnId"] == turn["turn"]["id"]
				&& params["item"]["type"] == "userMessage"
			{
				let content = params["item"]["content"]
					.as_array()
					.expect("native notification media fixture");
				assert_eq!(content.iter().any(|part| part["type"] == "image"), !omit_media);
				assert!(content.iter().any(|part| part["text"] == text));
				user_events.insert(method.clone());
			}
			if let ServerEvent::Notification { method, params } = event
				&& method == "turn/completed"
				&& params["threadId"] == thread_id
				&& params["turn"]["id"] == turn["turn"]["id"]
			{
				assert_eq!(params["turn"]["status"], "completed");
				break;
			}
		}
		assert_eq!(user_events.len(), 2, "both user item notifications must be observed");
	}
	assert_eq!(requests.load(Ordering::Acquire), 2);
	read_history(client, thread_id, requests).await;
}

async fn read_history(
	client: &AppServerClient,
	thread_id: &str,
	requests: &std::sync::atomic::AtomicUsize,
) -> Vec<decodex_protocol::ChiefTimelineEntry> {
	let mut cursor = None;
	let mut entries = Vec::new();
	let mut cursors = HashSet::new();
	for _ in 0..20 {
		let page = client
			.thread_timeline_page(thread_id, cursor.as_deref(), 1)
			.await
			.expect("native history fixture operation");
		let projected = crate::chief::timeline::project(thread_id, &page)
			.expect("native history fixture operation");
		assert_eq!(projected.entries.len(), 1);
		entries.extend(projected.entries);
		cursor = projected.next_cursor;
		let Some(value) = cursor.as_ref() else { break };
		assert!(cursors.insert(value.clone()), "native cursor cycle");
	}
	assert!(cursor.is_none(), "bounded history must reach its beginning");
	assert_eq!(entries.len(), 8, "two starts, user messages, answers and completions");
	let text = serde_json::to_string(&entries).expect("native history fixture operation");
	assert!(text.contains("First native history input"));
	assert!(text.contains("Second native history input"));
	assert!(text.contains("Native bridge answer"));
	qualify_media(client, thread_id, &entries).await;
	assert_eq!(requests.load(Ordering::Acquire), 2, "history reads started model work");
	entries
}

async fn qualify_media(
	client: &AppServerClient,
	thread_id: &str,
	entries: &[decodex_protocol::ChiefTimelineEntry],
) {
	use base64::Engine as _;
	use decodex_protocol::{ChiefMediaRequest, ChiefMediaResult, ChiefTimelineContent, EntityId};
	let (turn_id, item_id, index) = entries
		.iter()
		.find_map(|entry| {
			let ChiefTimelineContent::Item { turn_id, item_id, attachments, .. } = &entry.content
			else {
				return None;
			};
			attachments.first().map(|media| (turn_id, item_id, media.index))
		})
		.expect("native user image descriptor");
	assert_eq!(index, 1, "image retains its original user content index");
	let request = ChiefMediaRequest {
		work_id: EntityId::new("work").expect("native history fixture operation"),
		thread_id: EntityId::new(thread_id).expect("native history fixture operation"),
		turn_id: EntityId::new(turn_id).expect("native history fixture operation"),
		item_id: EntityId::new(item_id).expect("native history fixture operation"),
		index,
		offset: 0,
		fingerprint: None,
	};
	let result = crate::chief::timeline::media::read(
		|| {
			let client = client.clone();
			async move {
				Some(crate::chief_usage_estimate::Source {
					client,
					key: crate::chief_usage_estimate::SourceKey {
						history_revision: 0,
						generation: decodex_core::ProcessGenerationId::new(
							"10000000-0000-4000-8000-000000000002",
						)
						.expect("native history fixture operation"),
						account: decodex_core::AccountId::new(
							"10000000-0000-4000-8000-000000000001",
						)
						.expect("native history fixture operation"),
						revision: 1,
						thread: thread_id.into(),
						work: "work".into(),
					},
				})
			}
		},
		&request,
	)
	.await;
	let ChiefMediaResult::Available { bytes, total_bytes, mime_type, .. } = result else {
		panic!("native source image did not resolve through the retained bridge: {result:?}");
	};
	assert_eq!(mime_type, "image/png");
	assert_eq!(
		bytes,
		base64::engine::general_purpose::STANDARD
			.decode(PNG)
			.expect("native history fixture operation")
	);
	assert_eq!(total_bytes as usize, bytes.len());
}

async fn serve(listener: tokio::net::TcpListener, requests: Arc<std::sync::atomic::AtomicUsize>) {
	serve_with_bodies(listener, requests, None).await;
}

async fn serve_with_bodies(
	listener: tokio::net::TcpListener,
	requests: Arc<std::sync::atomic::AtomicUsize>,
	bodies: Option<Arc<std::sync::Mutex<Vec<Value>>>>,
) {
	serve_with_usage(listener, requests, bodies, |_| json!({"extra":{"fixture":"native-usage"},"input_tokens":0,"output_tokens":0,"total_tokens":0})).await;
}

async fn serve_with_usage(
	listener: tokio::net::TcpListener,
	requests: Arc<std::sync::atomic::AtomicUsize>,
	bodies: Option<Arc<std::sync::Mutex<Vec<Value>>>>,
	usage: fn(usize) -> Value,
) {
	serve_with_text(listener, requests, bodies, usage, |_| "Native bridge answer").await;
}

async fn serve_with_text(
	listener: tokio::net::TcpListener,
	requests: Arc<std::sync::atomic::AtomicUsize>,
	bodies: Option<Arc<std::sync::Mutex<Vec<Value>>>>,
	usage: fn(usize) -> Value,
	text: fn(usize) -> &'static str,
) {
	serve_with_output(listener, requests, bodies, usage, move |serial| {
		json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":text(serial)}]})
	}).await;
}

async fn serve_with_output(
	listener: tokio::net::TcpListener,
	requests: Arc<std::sync::atomic::AtomicUsize>,
	bodies: Option<Arc<std::sync::Mutex<Vec<Value>>>>,
	usage: fn(usize) -> Value,
	output: impl Fn(usize) -> Value,
) {
	use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};
	while let Ok((socket, _)) = listener.accept().await {
		let mut socket = tokio::io::BufReader::new(socket);
		let mut length = 0;
		loop {
			let mut line = String::new();
			assert!(
				socket.read_line(&mut line).await.expect("HTTP header") > 0,
				"truncated HTTP header"
			);
			if line == "\r\n" {
				break;
			}
			if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
				length = value.trim().parse::<usize>().expect("HTTP content length");
			}
		}
		assert!((1..=2 * 1024 * 1024).contains(&length));
		let mut body = vec![0; length];
		socket.read_exact(&mut body).await.expect("native history fixture operation");
		let body: Value = serde_json::from_slice(&body).expect("Responses request JSON");
		if let Some(bodies) = &bodies {
			bodies.lock().expect("fixture request bodies").push(body);
		}
		let serial = requests.fetch_add(1, Ordering::AcqRel);
		let id = format!("fixture-{serial}");
		let frames = [
			json!({"type":"response.created","response":{"id":id}}),
			json!({"type":"response.output_item.done","item":output(serial)}),
			json!({"type":"response.completed","response":{"id":id,"usage_metadata":{"amount":"0.12345678901234567890"},"usage":usage(serial)}}),
		];
		let data = frames
			.iter()
			.map(|v| {
				format!("event: {}\ndata: {v}\n\n", v["type"].as_str().expect("SSE event type"))
			})
			.collect::<String>();
		let response = format!(
			"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}",
			data.len()
		);
		socket
			.get_mut()
			.write_all(response.as_bytes())
			.await
			.expect("native history fixture operation");
	}
}
