//! Qualify native context accounting after responses without usable usage.
use std::{
	env, fs,
	sync::{Mutex, atomic::AtomicUsize},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use tokio::{net::TcpListener, time};

use super::{SUMMARY, start_turn};
use crate::account_launch::agent_process::native_tests::{
	self, Arc, Duration, NativeSession, ServerEvent, Value,
};

fn response_frames(serial: usize, compact: bool, event: &str, with_usage: bool) -> Vec<Value> {
	let id = format!("unreported-{serial}");
	let partial = matches!(serial, 1 | 2);
	let item = if partial {
		let reasoning = "r".repeat(12_000);
		// Match the native fixture's envelope so its estimate is about 3,000 tokens.
		let encrypted = STANDARD.encode(format!("{}{reasoning}", "b".repeat(550)));

		serde_json::json!({"id":id,"type":"reasoning","summary":[],"encrypted_content":encrypted,"content":[{"type":"reasoning_text","text":reasoning}]})
	} else if compact {
		serde_json::json!({"type":"compaction","encrypted_content":SUMMARY})
	} else {
		serde_json::json!({"type":"message","role":"assistant","id":id,"content":[{"type":"output_text","text":"Done"}]})
	};
	let usage = if partial && !with_usage {
		Value::Null
	} else {
		let tokens = if partial { 15 } else { 80 };

		serde_json::json!({"input_tokens":tokens,"output_tokens":0,"total_tokens":tokens})
	};

	vec![
		serde_json::json!({"type":"response.created","response":{"id":id}}),
		serde_json::json!({"type":"response.output_item.done","item":item}),
		serde_json::json!({"type":if partial {event} else {"response.completed"},"response":{"id":id,"incomplete_details":{"reason":"max_output_tokens"},"usage":usage}}),
	]
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native context qualification"]
async fn installed_unreported_reasoning_triggers_compaction_before_continuation() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");

	for (event, with_usage) in [
		("response.incomplete", true),
		("response.incomplete", false),
		("response.completed", false),
	] {
		let home = tempfile::tempdir().unwrap();
		let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
		let address = listener.local_addr().unwrap();
		let bodies = Arc::new(Mutex::new(Vec::<Value>::new()));
		let captured = bodies.clone();
		let backend = tokio::spawn(native_tests::serve_fixture_frames(
			listener,
			Arc::new(AtomicUsize::new(0)),
			None,
			Some(bodies.clone()),
			move |serial| {
				let compact = captured.lock().unwrap().last().unwrap()["input"]
					.as_array()
					.unwrap()
					.iter()
					.any(|item| item["type"] == "compaction_trigger");

				response_frames(serial, compact, event, with_usage)
			},
		));

		fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\nmodel_auto_compact_token_limit = 5000\ncli_auth_credentials_store = \"file\"\n[features]\nremote_compaction_v2 = false\nenable_request_compression = false\n[model_providers.fixture]\nname = \"OpenAI\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\nstream_max_retries = 0\n")).unwrap();

		time::timeout(Duration::from_secs(60), async {
			let mut session = NativeSession::start(&binary, home.path());
			let started = session.client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only"})).await.unwrap();
			let thread = started["thread"]["id"].as_str().unwrap().to_owned();

			for (index, input) in ["INITIAL", "PARTIAL_ONE", "PARTIAL_TWO", "CONTINUE"].iter().enumerate() {
				start_turn(&session.client, &thread, input).await;

				loop {
					let ServerEvent::Notification { method, params } = session.events.recv().await.unwrap() else { continue };
					if params["threadId"] != thread || method != "turn/completed" { continue; }

					let expected = if matches!(index, 1 | 2) && event == "response.incomplete" { "failed" } else { "completed" };

					assert_eq!(params["turn"]["status"], expected, "{event}, usage={with_usage}");
					break;
				}
			}

			let bodies = bodies.lock().unwrap();
			let triggers = bodies.iter().map(|body| {
				body["input"].as_array().unwrap().iter()
					.filter(|item| item["type"] == "compaction_trigger").count()
			}).collect::<Vec<_>>();

			assert_eq!(triggers, [0, 0, 0, 1, 0], "{event}, usage={with_usage}: compact the unreported reasoning before continuing");
			assert!(bodies[3]["input"].as_array().unwrap().iter().any(|item| item["type"] == "reasoning"));
			assert!(bodies[4]["input"].as_array().unwrap().iter().any(|item| item["type"] == "compaction" && item["encrypted_content"] == SUMMARY));
		}).await.expect("unreported context deadline");

		backend.abort();
	}
}
