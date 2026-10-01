//! Qualify native prompt-audio preparation with an isolated provider.
use std::{
	env, fs,
	sync::{Mutex, atomic::AtomicUsize},
};

use tokio::{net::TcpListener, time};

use crate::account_launch::agent_process::native_tests::*;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native audio preparation"]
async fn installed_native_replaces_empty_tool_audio_without_losing_other_output() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");

	assert!(std::path::Path::new(&binary).is_absolute());

	let home = tempfile::tempdir().expect("native audio fixture");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("fixture listener");
	let address = listener.local_addr().expect("fixture address");
	let requests = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(serve_fixture(
		listener,
		requests.clone(),
		None,
		Some(bodies.clone()),
		Some(serde_json::json!({"input_tokens":0,"output_tokens":0,"total_tokens":0})),
		|serial| {
			if serial == 0 {
				serde_json::json!({"type":"function_call","name":"audio_fixture","arguments":"{}","call_id":"audio"})
			} else {
				serde_json::json!({"type":"message","role":"assistant","id":"done","content":[{"type":"output_text","text":"Done"}]})
			}
		},
	));

	fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[model_providers.fixture]\nname = \"Isolated audio fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).expect("fixture config");

	let mut session = NativeSession::start(&binary, home.path());

	time::timeout(Duration::from_secs(60), async {
		let started = session.client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only","dynamicTools":[{"name":"audio_fixture","description":"Return fixture audio","inputSchema":{"type":"object","properties":{}}}]})).await.expect("native thread");
		let thread = started["thread"]["id"].as_str().expect("thread id");

		session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Read the fixture output."}]})).await.expect("native turn");

		let mut answered = false;

		loop {
			match session.events.recv().await.expect("native event") {
				ServerEvent::Request { id, method, .. } => {
					assert_eq!(method, "item/tool/call");
					assert!(!answered);

					session.client.respond(id,serde_json::json!({"contentItems":[{"type":"inputText","text":"before-audio"},{"type":"inputAudio","audioUrl":"data:audio/wav;base64,"},{"type":"inputText","text":"after-audio"}],"success":true})).await.expect("fixture tool response");

					answered = true;
				},
				ServerEvent::Notification { method, params } if method == "turn/completed" => {
					assert_eq!(params["turn"]["status"], "completed");

					break;
				},
				_ => {},
			}
		}

		assert!(answered);

		let bodies = bodies.lock().expect("fixture bodies");

		assert_eq!(bodies.len(), 2);

		let output = bodies[1]["input"].as_array().expect("model input").iter()
			.find(|item| item["type"] == "function_call_output" && item["call_id"] == "audio")
			.expect("tool output");

		assert_eq!(output["output"], serde_json::json!([
			{"type":"input_text","text":"before-audio"},
			{"type":"input_text","text":"audio content omitted because it could not be processed"},
			{"type":"input_text","text":"after-audio"}
		]));
	}).await.expect("native audio deadline");

	drop(session);

	backend.abort();
}
