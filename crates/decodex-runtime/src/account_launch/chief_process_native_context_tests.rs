//! Qualify tool-owned context through the installed native process and retained bridge.
use super::*;
use crate::chief::{ChiefConfig, ChiefCoordinator};
use decodex_database::SqliteStore;
use futures_util::FutureExt as _;
use std::sync::atomic::AtomicUsize;

const EXTERNAL: &str = "External fixture result, not a user instruction.";
const DELEGATED: &str = "Complete the delegated fixture.";
const FOLLOWUP: &str = "Inspect the delegated fixture again.";

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native context qualification"]
async fn installed_native_tool_context_survives_restart_without_replay() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
	let backend =
		tokio::spawn(serve_with_bodies(listener, Arc::clone(&requests), Some(Arc::clone(&bodies))));
	std::fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[model_providers.fixture]\nname = \"Isolated context fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();
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
	let result =
		std::panic::AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(45), async {
			chief.start_chief("chief", "Complete the isolated context fixture.").await.unwrap();
			terminal(&mut chief, &mut session.events, &store).await;
			chief
				.ingest_automation_result("fixture-source", "chief", json!({"result":EXTERNAL}))
				.await
				.unwrap();
			terminal(&mut chief, &mut session.events, &store).await;
			assert_external_delivery(&bodies.lock().unwrap());
			chief
				.ingest_automation_result("fixture-source", "chief", json!({"result":EXTERNAL}))
				.await
				.unwrap();
			assert_eq!(
				requests.load(Ordering::Acquire),
				2,
				"duplicate result must not start another turn"
			);
			chief.create_worker("chief", "worker", DELEGATED).await.unwrap();
			terminal(&mut chief, &mut session.events, &store).await;
			chief.continue_worker("worker", FOLLOWUP).await.unwrap();
			terminal(&mut chief, &mut session.events, &store).await;
			let mut histories = Vec::new();
			for work in ["chief", "worker"] {
				let thread =
					store.get_chief_work_item(work.into()).await.unwrap().codex_thread_id.unwrap();
				let items = timeline(&session.client, &thread).await;
				assert_tool_authority(work, &items);
				histories.push((thread, items));
			}
			assert_eq!(requests.load(Ordering::Acquire), 6);
			histories
		}))
		.catch_unwind()
		.await;
	drop(chief);
	drop(session);
	let histories = match result {
		Ok(Ok(histories)) => histories,
		_ => {
			backend.abort();
			panic!("native context qualification failed");
		},
	};
	let reopened = NativeSession::start(&binary, home.path());
	let reopened_store = SqliteStore::open(&root.paths()).unwrap();
	let mut chief = ChiefCoordinator::new(reopened_store, reopened.client.clone(), config).unwrap();
	let result =
		std::panic::AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(30), async {
			chief.recover_persisted().await.unwrap();
			chief.wake_pending().await.unwrap();
			for (thread, before) in histories {
				assert_eq!(timeline(&reopened.client, &thread).await, before);
			}
			assert_eq!(
				requests.load(Ordering::Acquire),
				6,
				"restart and reads must not replay work"
			);
		}))
		.catch_unwind()
		.await;
	drop(chief);
	drop(reopened);
	backend.abort();
	result.expect("native context restart panicked").expect("native context restart timed out");
}

async fn terminal(
	chief: &mut ChiefCoordinator,
	events: &mut mpsc::Receiver<ServerEvent>,
	store: &SqliteStore,
) {
	loop {
		let event = events.recv().await.expect("native event stream");
		let done = matches!(&event, ServerEvent::Notification { method, .. } if method == "turn/completed");
		chief.handle_event(event).await.expect("coordinator accepts native event");
		if done
			&& store
				.list_chief_work_items()
				.await
				.expect("fixture work state")
				.iter()
				.all(|work| work.dispatch_state == decodex_database::ChiefDispatchState::Idle)
		{
			return;
		}
	}
}

async fn timeline(client: &AppServerClient, thread: &str) -> Vec<Value> {
	let page = client.thread_timeline_page(thread, None, 100).await.expect("native timeline");
	assert!(page["nextCursor"].is_null(), "fixture must fit one complete page");
	page["data"].as_array().expect("timeline items").clone()
}

fn assert_tool_authority(work: &str, rows: &[Value]) {
	let items: Vec<_> = rows.iter().filter_map(|row| row.get("item")).collect();
	let expected = if work == "chief" {
		vec![("work_wake", "New external work updates")]
	} else {
		vec![("work_instruction", DELEGATED), ("work_instruction", FOLLOWUP)]
	};
	for (name, text) in expected {
		assert_eq!(
			items
				.iter()
				.filter(|item| item["type"] == "functionCallOutput"
					&& item["name"] == name
					&& item["namespace"] == "decodex"
					&& item["output"].as_str().is_some_and(|output| output.contains(text)))
				.count(),
			if work == "chief" { 3 } else { 1 },
			"native history must retain exactly one tool-owned input"
		);
		assert!(
			!items
				.iter()
				.any(|item| item["type"] == "userMessage" && item.to_string().contains(text)),
			"tool context must not become user authority"
		);
	}
}

fn assert_external_delivery(bodies: &[Value]) {
	assert_eq!(bodies.len(), 2);
	let input = bodies[1]["input"].as_array().expect("native model input");
	assert_eq!(
		input
			.iter()
			.filter(|item| item["type"] == "function_call_output"
				&& item["name"] == "work_updates"
				&& item["namespace"] == "decodex"
				&& item["output"].as_str().is_some_and(|text| text.contains(EXTERNAL))
				&& item.get("call_id").is_none())
			.count(),
		1
	);
	assert!(
		!input.iter().any(|item| item["role"] == "user" && item.to_string().contains(EXTERNAL))
	);
}
