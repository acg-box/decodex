//! Native realtime history crosses the retained bridge and Chief without replay.
use super::*;
use crate::chief::{ChiefConfig, ChiefCoordinator};
use decodex_database::SqliteStore;
use decodex_protocol::ChiefTimelineContent as Content;
use futures_util::SinkExt as _;
use std::sync::atomic::AtomicUsize;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native realtime qualification"]
async fn installed_native_realtime_history_survives_chief_and_cold_bridge() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().unwrap();
	let responses = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = responses.local_addr().unwrap();
	let websocket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let realtime_address = websocket.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve_with_text(
		responses,
		requests.clone(),
		None,
		|_| json!({"input_tokens":10,"output_tokens":2,"total_tokens":12}),
		|serial| if serial == 0 { "Seed" } else { "::codex-realtime-inline{}\nShared artifact" },
	));
	let (speech_tx, speech_rx) = tokio::sync::oneshot::channel();
	let (done_tx, done_rx) = tokio::sync::oneshot::channel();
	let realtime = tokio::spawn(serve_realtime(websocket, speech_rx, done_rx));
	std::fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\nexperimental_realtime_ws_base_url = \"ws://{realtime_address}\"\nexperimental_realtime_ws_startup_context = \"\"\n[features]\nrealtime_conversation = true\n[realtime]\nversion = \"v2\"\n[model_providers.fixture]\nname = \"Isolated realtime fixture\"\nbase_url = \"http://{address}\"\nexperimental_bearer_token = \"fixture-only-not-a-credential\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();
	let root =
		decodex_core::DecodexRoot::new(home.path().canonicalize().unwrap().join("state")).unwrap();
	root.paths().ensure_layout().unwrap();
	let store = SqliteStore::open(&root.paths()).unwrap();
	let mut config =
		ChiefConfig::new("gpt-5.6-sol".into(), "medium".into(), home.path().display().to_string());
	config.sandbox = "read-only".into();
	config.approval_policy = json!("never");
	let mut session = NativeSession::start(&binary, home.path());
	let mut chief =
		ChiefCoordinator::new(store.clone(), session.client.clone(), config.clone()).unwrap();
	let mut completed = Vec::new();
	let (thread, before) = tokio::time::timeout(Duration::from_secs(30), async {
		chief.start_chief("chief", "Seed native timeline").await.unwrap();
		drain(&mut chief, &mut session.events, "turn/completed", 1, &mut completed).await;
		let thread = store.get_chief_work_item("chief".into()).await.unwrap().codex_thread_id.unwrap();
		session.client.request("thread/realtime/start",json!({"threadId":thread,"prompt":"fixture","version":"v2","outputModality":"text","transport":{"type":"websocket"}})).await.unwrap();
		speech_tx.send(()).unwrap();
		drain(&mut chief, &mut session.events, "thread/realtime/transcript/delta", 2, &mut completed).await;
		chief.continue_worker("chief", "Typed input during speech").await.unwrap();
		drain(&mut chief, &mut session.events, "turn/completed", 1, &mut completed).await;
		done_tx.send(()).unwrap();
		drain(&mut chief, &mut session.events, "thread/realtime/closed", 1, &mut completed).await;
		let page = session.client.thread_timeline_page(&thread, None, 30).await.unwrap();
		let stored: Vec<_> = page["data"].as_array().unwrap().iter().filter(|row| row["type"] == "realtime").map(|row| row["item"].clone()).collect();
		assert_eq!(completed, stored);
		assert_projection(&thread, &page);
		assert_enriched(&session.client, &store, &thread).await;
		(thread, page)
	}).await.unwrap();
	drop(chief);
	drop(session);
	drop(store);
	let store = SqliteStore::open(&root.paths()).unwrap();
	let session = NativeSession::start(&binary, home.path());
	let mut chief = ChiefCoordinator::new(store.clone(), session.client.clone(), config).unwrap();
	tokio::time::timeout(Duration::from_secs(30), async {
		chief.recover_persisted().await.unwrap();
		let after = session.client.thread_timeline_page(&thread, None, 30).await.unwrap();
		assert_eq!(before, after);
		assert_projection(&thread, &after);
		assert_enriched(&session.client, &store, &thread).await;
	})
	.await
	.unwrap();
	assert_eq!(requests.load(Ordering::Acquire), 2, "recovery must not infer or replay input");
	realtime.await.unwrap();
	drop(chief);
	drop(session);
	backend.abort();
}

async fn assert_enriched(client: &AppServerClient, store: &SqliteStore, thread: &str) {
	let result = crate::chief::timeline::read(
		Some(store),
		|| {
			let client = client.clone();
			async move {
				Some(crate::chief_usage_estimate::Source {
					key: crate::chief_usage_estimate::SourceKey {
						generation: decodex_core::ProcessGenerationId::new(
							"10000000-0000-4000-8000-000000000002",
						)
						.expect("fixture generation"),
						account: decodex_core::AccountId::new(
							"10000000-0000-4000-8000-000000000001",
						)
						.expect("fixture account"),
						revision: 1,
						history_revision: client.history_revision(),
						thread: thread.into(),
						work: "chief".into(),
					},
					client,
				})
			}
		},
		None,
	)
	.await;
	let decodex_protocol::ChiefTimelineResult::Available { page, .. } = result else {
		panic!("native timeline enrichment failed: {result:?}");
	};
	assert!(page.entries.iter().any(|entry| matches!(&entry.content,
		Content::Promotion { resolved: Some(content), .. } if content.text.contains("Shared artifact"))));
}

async fn drain(
	chief: &mut ChiefCoordinator,
	events: &mut mpsc::Receiver<ServerEvent>,
	method: &str,
	mut count: usize,
	completed: &mut Vec<Value>,
) {
	while count > 0 {
		let event = events.recv().await.expect("native realtime event");
		if let ServerEvent::Notification { method: observed, params } = &event {
			assert_ne!(observed, "thread/realtime/error", "{params}");
			if observed == "thread/realtime/item/completed" {
				completed.push(params["item"].clone());
			}
			if observed == method {
				count -= 1;
			}
		}
		chief.handle_event(event).await.expect("Chief realtime event");
	}
}

fn assert_projection(thread: &str, page: &Value) {
	let page = crate::chief::timeline::project(thread, page).expect("native realtime projection");
	let speech: Vec<_> = page
		.entries
		.iter()
		.filter_map(|entry| match &entry.content {
			Content::Speech { role, text, .. } =>
				Some((entry.position, role.as_str(), text.as_str())),
			_ => None,
		})
		.collect();
	assert_eq!(
		speech.iter().map(|(_, role, text)| (*role, *text)).collect::<Vec<_>>(),
		vec![("assistant", "Speech before typed input"), ("user", "User speech")]
	);
	let typed = page.entries.iter().find(|entry| matches!(&entry.content, Content::Item { kind, text, .. } if kind == "userMessage" && text == "Typed input during speech")).expect("typed input");
	assert!(speech.iter().all(|(position, _, _)| *position < typed.position));
	assert_eq!(page.entries.iter().filter(|entry| matches!(&entry.content, Content::Promotion { presentation, .. } if presentation == "inlineMarkdown")).count(), 1);
	assert_eq!(page.entries.iter().filter(|entry| matches!(&entry.content, Content::VoiceBoundary { outcome: Some(outcome), .. } if outcome == "ended")).count(), 1);
}

async fn serve_realtime(
	listener: tokio::net::TcpListener,
	speech: tokio::sync::oneshot::Receiver<()>,
	done: tokio::sync::oneshot::Receiver<()>,
) {
	let (socket, _) = listener.accept().await.expect("realtime fixture connection");
	let mut socket =
		tokio_tungstenite::accept_async(socket).await.expect("realtime fixture handshake");
	socket
		.send(tokio_tungstenite::tungstenite::Message::Text(
			json!({"type":"session.updated","session":{"id":"voice-fixture"}}).to_string().into(),
		))
		.await
		.expect("session update");
	speech.await.expect("speech gate");
	for event in [
		json!({"type":"response.output_text.delta","delta":"Speech before typed input"}),
		json!({"type":"conversation.item.input_audio_transcription.delta","delta":"User speech"}),
	] {
		socket
			.send(tokio_tungstenite::tungstenite::Message::Text(event.to_string().into()))
			.await
			.expect("speech delta");
	}
	done.await.expect("speech completion gate");
	for event in [
		json!({"type":"response.output_text.done","text":"Speech before typed input"}),
		json!({"type":"conversation.item.input_audio_transcription.completed","transcript":"User speech"}),
	] {
		socket
			.send(tokio_tungstenite::tungstenite::Message::Text(event.to_string().into()))
			.await
			.expect("speech done");
	}
	socket.close(None).await.expect("normal realtime close");
}
