//! Opt-in installed-native qualification with a local Responses fixture and no credentials.
use super::*;
#[path = "chief_process_native_catalog_tests.rs"] mod catalog;
#[path = "chief_process_native_effort_tests.rs"] mod effort;
#[path = "chief_process_native_goal_tests.rs"] mod goals;
#[path = "chief_process_native_model_tests.rs"] mod models;
#[path = "chief_process_native_permission_tests.rs"] mod permissions;
#[path = "chief_process_native_plugin_tests.rs"] mod plugins;
#[path = "chief_process_native_reviewer_tests.rs"] mod reviewer;
#[path = "chief_process_native_steer_tests.rs"] mod steer;
use serde_json::json;
use std::{
	io::{BufRead, BufReader},
	process::{Child, Command, Stdio},
	sync::mpsc as sync_mpsc,
};

const PNG: &str =
	"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aB9sAAAAASUVORK5CYII=";

struct NativeChild(Child);

impl Drop for NativeChild {
	fn drop(&mut self) {
		let _ = self.0.kill();
		let _ = self.0.wait();
	}
}

#[tokio::test]
#[ignore = "requires explicit DECODEX_TEST_CODEX_BINARY; isolated native history qualification"]
async fn installed_native_history_reads_cross_retained_bridge_without_new_model_work() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let backend = tokio::spawn(serve(listener, Arc::clone(&requests)));
	std::fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[model_providers.fixture]\nname = \"Isolated history fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();
	let mut session = NativeSession::start(&binary, home.path());
	let (id, before) = tokio::time::timeout(Duration::from_secs(30), async {
		let client = &session.client;
		let started = client.thread_start(json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only"})).await.unwrap();
		let id = started["thread"]["id"].as_str().unwrap();
		assert!(matches!(client.thread_timeline_page(id, None, 30).await,
			Err(ClientError::Remote(error)) if error.code == -32601));
		let read = client.thread_read(json!({"threadId":id,"includeTurns":false})).await.unwrap();
		assert_eq!(read["thread"]["id"], id);
		assert_eq!(read["thread"]["historyMode"], "paginated");
		qualify_history(client, &mut session.events, id, &requests).await;
		(id.to_owned(), read_history(client, id, &requests).await)
	}).await.unwrap();
	drop(session);
	let reopened = NativeSession::start(&binary, home.path());
	let after = tokio::time::timeout(
		Duration::from_secs(30),
		read_history(&reopened.client, &id, &requests),
	)
	.await
	.unwrap();
	assert_eq!(
		serde_json::to_value(before).unwrap(),
		serde_json::to_value(after).unwrap(),
		"restart changed native history"
	);
	drop(reopened);
	backend.abort();
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
		writeln!(stdin, "{}", json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"decodex_native_history_test","version":"0.1"},"capabilities":{"experimentalApi":true,"optOutNotificationMethods":["rawResponseItem/completed"]}}})).expect("native session setup");
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
) {
	for text in ["First native history input", "Second native history input"] {
		let turn = client
			.turn_start(json!({"threadId":thread_id,"input":[{"type":"text","text":text},{"type":"image","url":format!("data:image/png;base64,{PNG}")}]}))
			.await
			.expect("native history fixture operation");
		loop {
			let event = events.recv().await.expect("native event stream");
			if let ServerEvent::Notification { method, params } = event
				&& method == "turn/completed"
				&& params["threadId"] == thread_id
				&& params["turn"]["id"] == turn["turn"]["id"]
			{
				assert_eq!(params["turn"]["status"], "completed");
				break;
			}
		}
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
	serve_with_effort(listener, requests, None).await;
}

async fn serve_with_effort(
	listener: tokio::net::TcpListener,
	requests: Arc<std::sync::atomic::AtomicUsize>,
	effort: Option<&str>,
) {
	serve_fixture(listener, requests, effort, None, None, |serial| json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Native bridge answer"}]})).await;
}

async fn serve_fixture(
	listener: tokio::net::TcpListener,
	requests: Arc<std::sync::atomic::AtomicUsize>,
	effort: Option<&str>,
	bodies: Option<Arc<std::sync::Mutex<Vec<Value>>>>,
	usage: Option<Value>,
	output: fn(usize) -> Value,
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
		if let Some(effort) = effort {
			assert_eq!(body["reasoning"]["effort"], effort);
			let metadata: Value = serde_json::from_str(
				body["client_metadata"]["x-codex-turn-metadata"].as_str().expect("turn metadata"),
			)
			.expect("metadata JSON");
			assert_eq!(metadata["turn_trigger"], "user");
			assert!(metadata["thread_id"].as_str().is_some_and(|id| !id.is_empty()));
			assert!(metadata["turn_id"].as_str().is_some_and(|id| !id.is_empty()));
		}
		if let Some(bodies) = &bodies {
			bodies.lock().expect("fixture bodies").push(body);
		}
		let serial = requests.fetch_add(1, Ordering::AcqRel);
		let id = format!("fixture-{serial}");
		let frames = [
			json!({"type":"response.created","response":{"id":id}}),
			json!({"type":"response.output_item.done","item":output(serial)}),
			json!({"type":"response.completed","response":{"id":id,"usage_metadata":{"amount":"0.12345678901234567890"},"usage":usage.clone().unwrap_or_else(||json!({"extra":{"fixture":"native-usage"},"input_tokens":0,"output_tokens":0,"total_tokens":0}))}}),
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
