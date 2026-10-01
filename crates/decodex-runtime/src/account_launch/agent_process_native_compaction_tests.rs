//! Qualify streamed compaction and cold continuation through the installed native bridge.
#[path = "agent_process_native_code_compaction_tests.rs"] mod code_metadata;

use super::*;

use std::sync::atomic::AtomicUsize;

const SUMMARY: &str = "isolated-compaction-checkpoint";

fn assert_checkpoint(bodies: &[Value], count: usize) {
	assert_eq!(bodies.len(), count);
	assert_eq!(
		bodies[1]["input"]
			.as_array()
			.expect("compaction request input")
			.iter()
			.filter(|item| item["type"] == "compaction_trigger")
			.count(),
		1
	);

	for request in &bodies[2..] {
		let input = request["input"].as_array().expect("continuation request input");

		assert!(!input.iter().any(|item| item["type"] == "compaction_trigger"));
		assert!(
			input
				.iter()
				.any(|item| item["type"] == "compaction" && item["encrypted_content"] == SUMMARY)
		);
		assert!(
			input
				.iter()
				.any(|item| item.to_string().contains("Remember the original user request."))
		);
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native compaction qualification"]
async fn installed_native_streams_auto_compaction_and_resumes_its_checkpoint() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");

	assert!(std::path::Path::new(&binary).is_absolute());

	{
		let home = tempfile::tempdir().expect("compaction fixture");
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let address = listener.local_addr().unwrap();
		let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
		let backend = tokio::spawn(serve_fixture_usage(
			listener,
			Arc::new(AtomicUsize::new(0)),
			None,
			Some(bodies.clone()),
			|serial| {
				let tokens = if serial == 0 { 250_000 } else { 100 };

				json!({"input_tokens":tokens,"output_tokens":0,"total_tokens":tokens})
			},
			|serial| {
				if serial == 1 {
					json!({"type":"compaction","encrypted_content":SUMMARY})
				} else {
					json!({"type":"message","role":"assistant","id":format!("reply-{serial}"),"content":[{"type":"output_text","text":"Done"}]})
				}
			},
		));
		let limit = 200_000;

		std::fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\nmodel_auto_compact_token_limit = {limit}\ncli_auth_credentials_store = \"file\"\n[features]\nremote_compaction_v2 = false\nenable_request_compression = false\n[model_providers.fixture]\nname = \"OpenAI\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();

		let mut session = NativeSession::start(&binary, home.path());

		tokio::time::timeout(Duration::from_secs(60), async {
			let started = session.client.thread_start(json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only"})).await.unwrap();
			let thread = started["thread"]["id"].as_str().unwrap().to_owned();

			start_turn(&session.client, &thread, "Remember the original user request.").await;

			assert!(completed(&mut session, &thread).await.is_empty());

			start_turn(&session.client, &thread, "Continue after automatic compaction.").await;

			let events = completed(&mut session, &thread).await;

			assert_eq!(events.len(), 2, "one started/completed pair: {events:?}");
			assert_eq!(events[0].0, "item/started");
			assert_eq!(events[1].0, "item/completed");
			assert_eq!(events[0].1, events[1].1, "same native compaction identity");

			assert_checkpoint(&bodies.lock().unwrap(), 3);
			drop(session);

			let mut reopened = NativeSession::start(&binary, home.path());

			reopened.client.thread_resume(json!({"threadId":thread,"excludeTurns":true})).await.unwrap();

			start_turn(&reopened.client, &thread, "Continue after restart.").await;

			assert!(completed(&mut reopened, &thread).await.is_empty());

			assert_checkpoint(&bodies.lock().unwrap(), 4);
		}).await.expect("native compaction deadline");

		backend.abort();
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native compaction failure qualification"]
async fn installed_native_preserves_prompt_before_compaction_error_and_after_restart() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");

	assert!(std::path::Path::new(&binary).is_absolute());

	let home = tempfile::tempdir().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve_fixture_usage(
		listener,
		requests.clone(),
		None,
		None,
		|_| json!({"input_tokens":250_000,"output_tokens":0,"total_tokens":250_000}),
		// An ordinary message is invalid output for remote compaction.
		|serial| json!({"type":"message","role":"assistant","id":format!("reply-{serial}"),"content":[{"type":"output_text","text":"Not a compaction checkpoint"}]}),
	));

	std::fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\nmodel_auto_compact_token_limit = 200000\ncli_auth_credentials_store = \"file\"\n[features]\nremote_compaction_v2 = false\nenable_request_compression = false\n[model_providers.fixture]\nname = \"OpenAI\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();

	const PROMPT: &str = "Keep this incoming prompt exactly once after compaction fails.";
	tokio::time::timeout(Duration::from_secs(60), async {
		let mut session = NativeSession::start(&binary, home.path());
		let started = session.client.thread_start(json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only"})).await.unwrap();
		let thread = started["thread"]["id"].as_str().unwrap().to_owned();

		start_turn(&session.client, &thread, "Build history for compaction.").await;
		completed(&mut session, &thread).await;

		let turn = session.client.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":PROMPT}]})).await.unwrap();
		let turn = turn["turn"]["id"].as_str().unwrap().to_owned();
		let mut inputs = 0;
		let mut errors = 0;

		loop {
			let ServerEvent::Notification { method, params } = session.events.recv().await.unwrap() else { continue };

			if params["threadId"] != thread { continue; }
			if method == "item/completed" && params["item"]["type"] == "userMessage" {
				assert!(params["item"].to_string().contains(PROMPT));

				inputs += 1;
			}
			if method == "error" {
				assert_eq!(inputs, 1, "input must precede the error: {params}");

				errors += 1;
			}
			if method == "turn/completed" && params["turn"]["id"] == turn {
				assert_eq!(params["turn"]["status"], "failed");

				break;
			}
		}

		assert_eq!(errors, 1);
		assert_eq!(requests.load(Ordering::Acquire), 2);

		drop(session);

		let reopened = NativeSession::start(&binary, home.path());
		let history = reopened.client.thread_read_turn(&thread, &turn).await.unwrap();
		let items = history["thread"]["turns"][0]["items"].as_array().unwrap();

		assert_eq!(items.iter().filter(|item| item["type"] == "userMessage" && item.to_string().contains(PROMPT)).count(), 1);
		assert_eq!(requests.load(Ordering::Acquire), 2, "cold read must not replay input");
	}).await.expect("native compaction failure deadline");

	backend.abort();
}

async fn start_turn(client: &AppServerClient, thread: &str, text: &str) {
	client
		.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":text}]}))
		.await
		.expect("native turn start");
}

async fn completed(session: &mut NativeSession, thread: &str) -> Vec<(String, Value)> {
	let mut compactions = Vec::new();

	loop {
		match session.events.recv().await.expect("native events") {
			ServerEvent::Notification { method, params } if params["threadId"] == thread => {
				if (method == "item/started" || method == "item/completed")
					&& params["item"]["type"] == "contextCompaction"
				{
					compactions
						.push((method.clone(), json!([params["turnId"], params["item"]["id"]])));
				}

				assert_ne!(method, "error", "native failure: {params}");

				if method == "turn/completed" {
					assert_eq!(params["turn"]["status"], "completed");

					return compactions;
				}
			},
			ServerEvent::Request { method, .. } => panic!("unexpected native request: {method}"),
			_ => {},
		}
	}
}
