//! Verify new and cold-resumed native response timing with a paced, local SSE provider.
use std::env;

use tokio::{
	io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader},
	net::TcpListener,
	time::{self, Duration},
};

use crate::account_launch::agent_process::native_tests::usage::{
	AgentConfig, AgentCoordinator, DecodexRoot, NativeSession, ServerEvent, SqliteStore, Value, fs,
};
use decodex_codex::app_server_client::ObservedOutputRate;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native streamed output rate qualification"]
async fn installed_native_generation_rate_survives_cold_resume_without_raw_notifications() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();

	fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[model_providers.fixture]\nname = \"Local speed fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();

	let backend = tokio::spawn(serve_output_rate(listener));
	let root = DecodexRoot::new(home.path().canonicalize().unwrap().join("state")).unwrap();

	root.paths().ensure_layout().unwrap();

	let store = SqliteStore::open(&root.paths()).unwrap();
	let mut config =
		AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), home.path().display().to_string());

	config.sandbox = "read-only".into();
	config.approval_policy = serde_json::json!("never");

	for cold in [false, true] {
		let mut session = NativeSession::start(&binary, home.path());
		let mut agent =
			AgentCoordinator::new(store.clone(), session.client.clone(), config.clone()).unwrap();

		time::timeout(Duration::from_secs(30), async {
			if cold {
				agent.recover_persisted().await.unwrap();
				agent.continue_worker("speed", "Reply once more.").await.unwrap();
			} else {
				agent.start_agent("speed", "Reply briefly.").await.unwrap();
			}

			let mut raw = 0;
			let turn = loop {
				let event = session.events.recv().await.unwrap();
				let done = if let ServerEvent::Notification { method, params } = &event {
					raw += usize::from(method == "rawResponse/completed");

					(method == "turn/completed")
						.then(|| params["turn"]["id"].as_str().unwrap().to_owned())
				} else {
					None
				};

				agent.handle_event(event).await.unwrap();

				if let Some(turn) = done {
					break turn;
				}
			};

			assert_eq!(
				raw,
				usize::from(!cold),
				"pin distinguishes new-thread and resumed capabilities"
			);

			let thread =
				store.get_agent_work_item("speed".into()).await.unwrap().codex_thread_id.unwrap();
			let event = store
				.read_agent_output_rate_observation("speed".into(), thread)
				.await
				.unwrap()
				.expect("stream timing saved");
			let value: Value = serde_json::from_str(&event.payload).unwrap();

			assert_eq!(
				value["turnId"], turn,
				"cold resume measures a new response, not the retained previous one"
			);

			let rate: ObservedOutputRate =
				serde_json::from_value(value["observedOutputRate"].clone()).unwrap();

			assert_eq!(rate.output_tokens, 120);
			assert!(
				(300..=2_000).contains(&rate.duration_ms),
				"paced generation interval: {}ms",
				rate.duration_ms
			);
		})
		.await
		.unwrap();
	}

	backend.await.unwrap();
}

async fn serve_output_rate(listener: TcpListener) {
	for serial in 0..2 {
		let (socket, _) =
			listener.accept().await.expect("Accept the local output-rate fixture request.");
		let mut socket = BufReader::new(socket);
		let mut length = 0;

		loop {
			let mut line = String::new();

			assert!(
				socket.read_line(&mut line).await.expect("Read the fixture request headers.") > 0
			);

			if line == "\r\n" {
				break;
			}

			if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
				length =
					n.trim().parse::<usize>().expect("Parse the fixture Content-Length header.");
			}
		}

		assert!((1..=2 * 1_024 * 1_024).contains(&length));

		socket
			.read_exact(&mut vec![0; length])
			.await
			.expect("Read the complete fixture request body.");

		let reasoning =
			serde_json::json!({"id":format!("reasoning-{serial}"),"type":"reasoning","summary":[]});
		let message = serde_json::json!({"id":format!("message-{serial}"),"type":"message","role":"assistant","content":[{"type":"output_text","text":"Measured reply"}]});
		let chunks = [
			vec![serde_json::json!({"type":"response.created","response":{"id":format!("response-{serial}")}}), serde_json::json!({"type":"response.output_item.added","item":reasoning})],
			vec![serde_json::json!({"type":"response.output_item.done","item":reasoning}), serde_json::json!({"type":"response.output_item.added","item":{"id":format!("message-{serial}"),"type":"message","role":"assistant","content":[]}}), serde_json::json!({"type":"response.output_text.delta","item_id":format!("message-{serial}"),"delta":"Measured reply"})],
			vec![serde_json::json!({"type":"response.output_item.done","item":message}), serde_json::json!({"type":"response.completed","response":{"id":format!("response-{serial}"),"usage":{"input_tokens":100,"output_tokens":120,"total_tokens":220,"output_tokens_details":{"reasoning_tokens":80}}}})],
		].map(|frames| frames.iter().map(|frame| format!("event: {}\ndata: {frame}\n\n", frame["type"].as_str().expect("Read the fixture SSE event type."))).collect::<String>());
		let length: usize = chunks.iter().map(String::len).sum();

		socket.get_mut().write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n").as_bytes()).await.expect("Write the paced fixture response.");

		for chunk in chunks {
			socket
				.get_mut()
				.write_all(chunk.as_bytes())
				.await
				.expect("Write the paced fixture response.");
			socket.get_mut().flush().await.expect("Write the paced fixture response.");

			time::sleep(Duration::from_millis(180)).await;
		}
	}
}
