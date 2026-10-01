//! Opt-in real app-server qualification with a local, deterministic Responses backend.
use std::{
	env, fs,
	panic::AssertUnwindSafe,
	sync::{Arc, Mutex},
	time::Duration,
};

use futures_util::FutureExt as _;
use tokio::{
	io::AsyncReadExt as _,
	net::{TcpListener, TcpStream},
	process::Command,
	sync::mpsc::Receiver,
	time,
};

use crate::agent::tests::{
	AgentConfig, AgentCoordinator, AppServerClient, AsyncWriteExt as _, EnqueueAgentEvent,
	ServerEvent, SqliteStore, Value,
};
use decodex_core::DecodexRoot;
use decodex_database::AgentDispatchState;

pub(super) async fn read_http_body(socket: &mut TcpStream) -> Value {
	let mut bytes = Vec::new();
	let mut buffer = [0; 8_192];

	loop {
		let count = socket.read(&mut buffer).await.unwrap();

		assert!(count > 0);

		bytes.extend_from_slice(&buffer[..count]);

		assert!(bytes.len() < 2 * 1_024 * 1_024);

		if let Some(start) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
			let headers = String::from_utf8_lossy(&bytes[..start]);
			let length = headers
				.lines()
				.find_map(|line| {
					line.to_ascii_lowercase()
						.strip_prefix("content-length:")
						.map(|n| n.trim().parse::<usize>().unwrap())
				})
				.unwrap();

			if bytes.len() >= start + 4 + length {
				return serde_json::from_slice(&bytes[start + 4..start + 4 + length]).unwrap();
			}
		}
	}
}

fn response_item(body: &Value, serial: usize, witness: &Mutex<bool>) -> Value {
	let input = body["input"].as_array().unwrap();

	if let Some(output) = input
		.iter()
		.find(|v| v["type"] == "function_call_output" && v["call_id"] == "read-reference")
	{
		assert!(
			output.to_string().contains("NATIVE_TARGET_EVIDENCE"),
			"actual tool output lacks target evidence: {output}"
		);

		*witness.lock().unwrap() = true;

		return message(serial, "REFERENCE_READ_DONE");
	}

	let texts: Vec<_> = input
		.iter()
		.filter_map(|v| v["content"].as_array())
		.flatten()
		.filter_map(|v| v["text"].as_str())
		.collect();

	if texts.iter().any(|text| text.contains("REFERENCE_TRIGGER")) {
		let metadata = texts
			.iter()
			.find_map(|text| text.strip_prefix("User-selected task references: "))
			.expect("native prompt lost typed references");
		let references: Value = serde_json::from_str(metadata.lines().next().unwrap()).unwrap();

		return serde_json::json!({"type":"function_call","id":"call-reference","call_id":"read-reference","name":"agent_read_work","arguments":serde_json::json!({"id":references[0]["workId"],"threadId":references[0]["threadId"]}).to_string()});
	}

	message(
		serial,
		if body.to_string().contains("TARGET_EVIDENCE_REQUEST") {
			"NATIVE_TARGET_EVIDENCE"
		} else {
			"READY"
		},
	)
}

fn message(serial: usize, text: &str) -> Value {
	serde_json::json!({"type":"message","role":"assistant","id":format!("message-{serial}"),"content":[{"type":"output_text","text":text}]})
}

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY pointing to an installed Codex binary"]
async fn native_task_reference_round_trip() {
	let binary = env::var("DECODEX_NATIVE_BINARY").expect("explicit native binary required");
	let directory = tempfile::tempdir().unwrap();
	let home = directory.path().canonicalize().unwrap();
	let observed = Arc::new(Mutex::new(false));
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let witness = observed.clone();
	let backend = tokio::spawn(async move {
		let mut serial = 0;

		while let Ok((mut socket, _)) = listener.accept().await {
			serial += 1;

			let body = read_http_body(&mut socket).await;
			let item = response_item(&body, serial, &witness);
			let id = format!("response-{serial}");
			let frames = [
				serde_json::json!({"type":"response.created","response":{"id":id}}),
				serde_json::json!({"type":"response.output_item.done","item":item}),
				serde_json::json!({"type":"response.completed","response":{"id":id,"usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0}}}),
			];
			let body = frames
				.iter()
				.map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap()))
				.collect::<String>();
			let response = format!(
				"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
				body.len()
			);

			socket.write_all(response.as_bytes()).await.unwrap();
		}
	});

	fs::write(home.join("config.toml"),format!(
		"model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\n[model_providers.fixture]\nname = \"Isolated reference fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n"
	)).unwrap();

	let root = DecodexRoot::new(home.join("product")).unwrap();

	root.paths().ensure_layout().unwrap();

	let store = SqliteStore::open(&root.paths()).unwrap();
	let mut command = Command::new(binary);

	command
		.arg("app-server")
		.current_dir(&home)
		.env_clear()
		.env("HOME", &home)
		.env("CODEX_HOME", &home)
		.env("PATH", "/usr/bin:/bin");

	let (client, mut events, mut process) = AppServerClient::spawn(&mut command).unwrap();
	let mut config =
		AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), home.display().to_string());

	config.sandbox = "read-only".into();
	config.approval_policy = serde_json::json!("never");

	let mut agent = AgentCoordinator::new(store.clone(), client, config).unwrap();
	let outcome = AssertUnwindSafe(time::timeout(Duration::from_secs(90), async {
		agent.initialize().await.unwrap();
		agent.start_agent("root", "ROOT_READY").await.unwrap();

		drain_work(&mut agent,&mut events,"root").await;

		agent.create_manager("root","child","CHILD_READY",None).await.unwrap();

		drain_work(&mut agent,&mut events,"child").await;

		let target = agent.create_worker("child","target","TARGET_EVIDENCE_REQUEST").await.unwrap();

		drain_work(&mut agent,&mut events,"target").await;

		let before = store.get_agent_work_item("target".into()).await.unwrap();
		let references = serde_json::json!([{"workId":"target","threadId":target.codex_thread_id,"title":"Selected target"}]);

		store.enqueue_agent_event(EnqueueAgentEvent {source_event_id:"reference-request".into(),work_item_id:"root".into(),event_kind:"user_message".into(),payload:serde_json::json!({"text":"REFERENCE_TRIGGER","source":"user","options":{"attachments":[],"taskReferences":references}}).to_string()}).await.unwrap();
		agent.wake_pending().await.unwrap();

		drain_work(&mut agent,&mut events,"root").await;

		assert!(*observed.lock().unwrap(),"native model continuation did not receive target evidence");
		assert_eq!(store.get_agent_work_item("target".into()).await.unwrap(),before,"reading changed target state");

		let reopened = SqliteStore::open(&root.paths()).unwrap();

		assert!(reopened.agent_has_task_reference("root".into(),"target".into(),target.codex_thread_id.unwrap()).await.unwrap());
	})).catch_unwind().await;

	process.shutdown().await.unwrap();
	backend.abort();
	outcome
		.expect("native reference fixture panicked")
		.expect("native reference round trip timed out");
}

async fn drain_work(agent: &mut AgentCoordinator, events: &mut Receiver<ServerEvent>, id: &str) {
	while agent.store.get_agent_work_item(id.into()).await.unwrap().dispatch_state
		!= AgentDispatchState::Idle
	{
		agent.handle_event(events.recv().await.expect("native event stream ended")).await.unwrap();
	}
}
