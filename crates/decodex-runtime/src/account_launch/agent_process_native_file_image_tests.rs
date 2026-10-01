//! File image references retain order and detail through native input and cold history.
use std::{
	env, fs,
	sync::{Mutex, atomic::AtomicUsize},
};

use tokio::{net::TcpListener, time};

use crate::account_launch::agent_process::native_tests::*;

fn assert_user_content(history: &Value, expected: &Value) {
	let item = history["thread"]["turns"][0]["items"]
		.as_array()
		.expect("history items")
		.iter()
		.find(|item| item["type"] == "userMessage")
		.expect("user message");

	assert_eq!(&item["content"], expected);
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated file image references"]
async fn installed_native_file_images_survive_cold_history() {
	for omit_media in [false, true] {
		qualify(omit_media).await;
	}
}

async fn qualify(omit_media: bool) {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native binary");
	let home = tempfile::tempdir().expect("image fixture");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("fixture listener");
	let address = listener.local_addr().expect("fixture address");
	let requests = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(serve_fixture(
		listener,
		requests.clone(),
		None,
		Some(bodies.clone()),
		None,
		|serial| serde_json::json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Native bridge answer"}]}),
	));

	fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[features]\nomit_app_server_notification_media = {omit_media}\n[model_providers.fixture]\nname = \"Isolated image fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).expect("fixture config");

	let input = serde_json::json!([
		{"type":"text","text":"Compare these references.","text_elements":[]},
		{"type":"image","fileId":"file_fixture_first","detail":"original"},
		{"type":"image","url":format!("data:image/png;base64,{PNG}"),"detail":"high"},
		{"type":"image","fileId":"file_fixture_last","detail":"low"}
	]);

	time::timeout(Duration::from_secs(45), async {
		let mut session = NativeSession::start(&binary, home.path());
		let started = session.client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only"})).await.expect("native thread");
		let thread = started["thread"]["id"].as_str().expect("thread ID").to_owned();
		let started = session.client.turn_start(serde_json::json!({"threadId":thread,"input":input})).await.expect("native turn");
		let turn = started["turn"]["id"].as_str().expect("turn ID").to_owned();
		let mut observed = 0;

		loop {
			match session.events.recv().await.expect("native event") {
				ServerEvent::Notification { method, params } => {
					assert_ne!(method, "error", "native error: {params}");

					if (method == "item/started" || method == "item/completed") && params["item"]["type"] == "userMessage" {
						observed += 1;

						if omit_media {
							assert!(!params.to_string().contains("file_fixture_"));
						} else {
							assert_eq!(params["item"]["content"], input);
						}
					}
					if method == "turn/completed" {
						assert_eq!(params["turn"]["status"], "completed");

						break;
					}
				},
				ServerEvent::Request { method, .. } => panic!("unexpected request: {method}"),
				_ => {},
			}
		}

		assert_eq!(observed, 2);

		let before = session.client.thread_read_turn(&thread, &turn).await.expect("live history");

		assert_user_content(&before, &input);
		drop(session);

		let mut reopened = NativeSession::start(&binary, home.path());
		let after = reopened.client.thread_read_turn(&thread, &turn).await.expect("cold history");

		assert_user_content(&after, &input);

		assert_eq!(requests.load(Ordering::Acquire), 1, "cold read must not replay input");

        reopened.client.thread_resume(serde_json::json!({"threadId":thread,"excludeTurns":true})).await.expect("cold resume");
        reopened.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Continue with the same recorded images."}]})).await.expect("cold continuation");

        loop {
            if let ServerEvent::Notification {method,params}=reopened.events.recv().await.expect("continuation event") {
                assert_ne!(method,"error","native continuation error: {params}");

                if method=="turn/completed" {
                    assert_eq!(params["turn"]["status"],"completed");

                    break;
                }
            }
        }

        assert_eq!(requests.load(Ordering::Acquire),2,"only one explicit continuation");

		let bodies = bodies.lock().expect("fixture bodies");
		let images: Vec<_> = bodies[0]["input"].as_array().expect("model input").iter()
			.filter(|item| item["role"] == "user")
			.filter_map(|item| item["content"].as_array()).flatten()
			.filter(|part| part["type"] == "input_image").collect();

		assert_eq!(images.len(), 3);
		assert_eq!(images[0]["file_id"], "file_fixture_first");
		assert_eq!(images[1]["image_url"], input[2]["url"]);
		assert_eq!(images[2]["file_id"], "file_fixture_last");

        let replayed: Vec<_> = bodies[1]["input"].as_array().expect("resumed model input").iter()
            .filter(|item|item["role"]=="user").filter_map(|item|item["content"].as_array()).flatten()
            .filter(|part|part["type"]=="input_image").collect();

        assert_eq!(replayed,images,"cold continuation must preserve prepared images without duplication");
		// Responses Lite strips detail on the model wire; display history keeps the hints.
		assert!(images.iter().all(|image| image.get("detail").is_none()));
	}).await.expect("file image deadline");

	backend.abort();
}
