//! Installed native -> retained bridge -> coordinator continuation qualification.
use super::*;
use crate::chief::{ChiefConfig, ChiefCoordinator};
use decodex_database::SqliteStore;
use futures_util::FutureExt as _;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};

const EXPLANATION: &str = "Review the isolated fixture scope.";
const STEER: &str = "Continue within the confirmed fixture scope.";

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native continuation qualification"]
async fn installed_native_continuation_uses_live_details_and_rejects_restart_authority() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(serve(listener, Arc::clone(&requests)));
	std::fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[model_providers.fixture]\nname = \"Isolated continuation fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();
	let root =
		decodex_core::DecodexRoot::new(home.path().canonicalize().unwrap().join("state")).unwrap();
	root.paths().ensure_layout().unwrap();
	let store = SqliteStore::open(&root.paths()).unwrap();
	let mut session = NativeSession::start(&binary, home.path());
	let mut config =
		ChiefConfig::new("gpt-5.6-sol".into(), "medium".into(), home.path().display().to_string());
	config.sandbox = "read-only".into();
	config.approval_policy = json!("never");
	let mut chief =
		ChiefCoordinator::new(store.clone(), session.client.clone(), config.clone()).unwrap();
	let outcome =
		std::panic::AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(45), async {
			chief.start_chief("chief", "Complete the isolated fixture.").await.unwrap();
			terminal(&mut chief, &mut session.events).await;
			let review = store.chief_misalignment("chief".into()).await.unwrap().unwrap();
			let (error, guard) = session
				.client
				.live_misalignment_review(&review.thread_id, &review.turn_id)
				.unwrap();
			assert_eq!(error["misalignment"]["detailedExplanation"], EXPLANATION);
			let token = crate::chief::misalignment::review_token(&review, &guard).unwrap();
			assert_history_omits_details(&session.client, &review).await;
			assert_eq!(requests.lock().unwrap().len(), 1, "readback must not start model work");
			assert!(
				chief
					.continue_misalignment("chief", review.clone(), "stale", "old-token")
					.await
					.is_err()
			);
			assert_eq!(requests.lock().unwrap().len(), 1);
			chief
				.continue_misalignment("chief", review, "explicit-fixture-consent", &token)
				.await
				.unwrap();
			terminal(&mut chief, &mut session.events).await;
			assert!(store.chief_misalignment("chief".into()).await.unwrap().is_none());
			assert_override(&requests.lock().unwrap());
			chief
				.enqueue_user_message(
					"chief",
					"next-fixture-input",
					"Leave the next fixture turn blocked.",
				)
				.await
				.unwrap();
			chief.wake_pending().await.unwrap();
			terminal(&mut chief, &mut session.events).await;
			store.chief_misalignment("chief".into()).await.unwrap().unwrap()
		}))
		.catch_unwind()
		.await;
	drop(chief);
	drop(session);
	let review = match outcome {
		Ok(Ok(review)) => review,
		_ => {
			backend.abort();
			panic!("native continuation qualification failed");
		},
	};
	let reopened = NativeSession::start(&binary, home.path());
	let mut chief = ChiefCoordinator::new(store.clone(), reopened.client.clone(), config).unwrap();
	let outcome =
		std::panic::AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(20), async {
			assert_history_omits_details(&reopened.client, &review).await;
			assert!(
				reopened
					.client
					.live_misalignment_review(&review.thread_id, &review.turn_id)
					.is_none()
			);
			assert!(
				chief
					.continue_misalignment("chief", review.clone(), "restarted", "old-token")
					.await
					.is_err()
			);
			assert_eq!(requests.lock().unwrap().len(), 3);
			assert_eq!(store.chief_misalignment("chief".into()).await.unwrap(), Some(review));
		}))
		.catch_unwind()
		.await;
	drop(chief);
	drop(reopened);
	backend.abort();
	outcome.expect("native restart qualification panicked").expect("native restart timed out");
}

async fn terminal(chief: &mut ChiefCoordinator, events: &mut mpsc::Receiver<ServerEvent>) {
	loop {
		let event = events.recv().await.expect("native event stream");
		let done = matches!(&event, ServerEvent::Notification { method, .. } if method == "turn/completed");
		chief.handle_event(event).await.expect("coordinator must accept native event");
		if done {
			return;
		}
	}
}

async fn assert_history_omits_details(
	client: &AppServerClient,
	review: &decodex_database::ChiefMisalignment,
) {
	let history = client
		.thread_read_turn(&review.thread_id, &review.turn_id)
		.await
		.expect("native continuation fixture");
	let turn = history["thread"]["turns"]
		.as_array()
		.expect("native continuation fixture")
		.iter()
		.find(|turn| turn["id"] == review.turn_id)
		.expect("native continuation fixture");
	assert_eq!(turn["status"], "failed");
	assert_eq!(turn["error"]["codexErrorInfo"], "misalignmentPolicyViolation");
	assert!(turn["error"]["misalignment"].is_null());
}

fn assert_override(requests: &[Value]) {
	assert_eq!(requests.len(), 2);
	let metadata = requests[1]
		.pointer("/client_metadata/x-codex-turn-metadata")
		.and_then(Value::as_str)
		.expect("native turn metadata");
	let metadata: Value = serde_json::from_str(metadata).expect("native continuation fixture");
	let metadata: Value = serde_json::from_str(
		metadata["misalignment_override"].as_str().expect("explicit native override metadata"),
	)
	.expect("native continuation fixture");
	assert!(metadata["timestamp"].as_u64().is_some_and(|time| time > 0));
	assert!(requests[1]["input"].as_array().expect("native continuation fixture").iter().any(
		|item| {
			item["role"] == "user"
				&& item["content"]
					.as_array()
					.is_some_and(|content| content.iter().any(|part| part["text"] == STEER))
		}
	));
}

async fn serve(listener: tokio::net::TcpListener, requests: Arc<Mutex<Vec<Value>>>) {
	while let Ok((socket, _)) = listener.accept().await {
		let mut socket = tokio::io::BufReader::new(socket);
		let mut length = 0;
		loop {
			let mut line = String::new();
			assert!(socket.read_line(&mut line).await.expect("native continuation fixture") > 0);
			if line == "\r\n" {
				break;
			}
			if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
				length = value.trim().parse::<usize>().expect("native continuation fixture");
			}
		}
		assert!((1..=2 * 1024 * 1024).contains(&length));
		let mut body = vec![0; length];
		socket.read_exact(&mut body).await.expect("native continuation fixture");
		let serial = {
			let mut requests = requests.lock().expect("native continuation fixture");
			requests.push(serde_json::from_slice(&body).expect("native continuation fixture"));
			requests.len()
		};
		let id = format!("fixture-{serial}");
		let mut frames = vec![json!({"type":"response.created","response":{"id":id}})];
		if serial == 2 {
			frames.push(json!({"type":"response.output_item.done","item":{"type":"message","role":"assistant","id":"done","content":[{"type":"output_text","text":"Done within scope."}]}}));
			frames.push(json!({"type":"response.completed","response":{"id":id,"usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0}}}));
		} else {
			frames.push(json!({"type":"response.failed","response":{"id":id,"status":"failed","error":{"code":"misalignment_policy_violation","message":"Fixture precaution.","misalignment":{"error_type":"fixture_scope","detailed_explanation":EXPLANATION,"steer":{"message":STEER}}}}}));
		}
		let data = frames
			.iter()
			.map(|frame| {
				format!(
					"event: {}\ndata: {frame}\n\n",
					frame["type"].as_str().expect("native continuation fixture")
				)
			})
			.collect::<String>();
		let response = format!(
			"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}",
			data.len()
		);
		socket.get_mut().write_all(response.as_bytes()).await.expect("native continuation fixture");
	}
}
