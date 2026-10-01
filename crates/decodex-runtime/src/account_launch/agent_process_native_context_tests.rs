//! Qualify tool-owned context through the installed native process and retained bridge.
use std::{
	env, fs,
	panic::AssertUnwindSafe,
	sync::{Mutex, atomic::AtomicUsize},
};

use futures_util::FutureExt as _;
use mpsc::Receiver;
use tokio::{net::TcpListener, time};

use crate::{
	account_launch::agent_process::native_tests::*,
	agent::{AgentConfig, AgentCoordinator, timeline::metrics},
};
use decodex_core::DecodexRoot;
use decodex_database::{AgentDispatchState, SqliteStore};
use decodex_protocol::AgentTimelineContent;

const EXTERNAL: &str = "External fixture result, not a user instruction.";
const DELEGATED: &str = "Complete the delegated fixture.";
const FOLLOWUP: &str = "Inspect the delegated fixture again.";

fn assert_tool_authority(work: &str, rows: &[Value]) {
	let items: Vec<_> = rows.iter().filter_map(|row| row.get("item")).collect();
	let expected = if work == "agent" {
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
			if work == "agent" { 3 } else { 1 },
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

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native context qualification"]
async fn installed_native_tool_context_survives_restart_without_replay() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");

	assert!(std::path::Path::new(&binary).is_absolute());

	let home = tempfile::tempdir().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(serve_fixture(
		listener,
		Arc::clone(&requests),
		None,
		Some(Arc::clone(&bodies)),
		None,
		|serial| serde_json::json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Native context answer"}]}),
	));

	fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[model_providers.fixture]\nname = \"Isolated context fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();

	let root = DecodexRoot::new(home.path().canonicalize().unwrap().join("state")).unwrap();

	root.paths().ensure_layout().unwrap();

	let store = SqliteStore::open(&root.paths()).unwrap();
	let mut session = NativeSession::start(&binary, home.path());
	let mut config =
		AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), home.path().display().to_string());

	config.sandbox = "read-only".into();
	config.approval_policy = serde_json::json!("never");

	let mut agent =
		AgentCoordinator::new(store.clone(), session.client.clone(), config.clone()).unwrap();
	let result = AssertUnwindSafe(time::timeout(Duration::from_secs(45), async {
		agent.start_agent("agent", "Complete the isolated context fixture.").await.unwrap();

		terminal(&mut agent, &mut session.events, &store).await;

		agent
			.ingest_automation_result(
				"fixture-source",
				"agent",
				serde_json::json!({"result":EXTERNAL}),
			)
			.await
			.unwrap();

		terminal(&mut agent, &mut session.events, &store).await;
		assert_external_delivery(&bodies.lock().unwrap());

		agent
			.ingest_automation_result(
				"fixture-source",
				"agent",
				serde_json::json!({"result":EXTERNAL}),
			)
			.await
			.unwrap();

		assert_eq!(
			requests.load(Ordering::Acquire),
			2,
			"duplicate result must not start another turn"
		);

		agent.create_worker("agent", "worker", DELEGATED).await.unwrap();

		terminal(&mut agent, &mut session.events, &store).await;

		agent.continue_worker("worker", FOLLOWUP).await.unwrap();

		terminal(&mut agent, &mut session.events, &store).await;

		let histories = checked_histories(&session.client, &store).await;

		assert_eq!(requests.load(Ordering::Acquire), 6);

		histories
	}))
	.catch_unwind()
	.await;

	drop(agent);
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
	let mut agent =
		AgentCoordinator::new(reopened_store.clone(), reopened.client.clone(), config).unwrap();
	let result = AssertUnwindSafe(time::timeout(Duration::from_secs(30), async {
		agent.recover_persisted().await.unwrap();
		agent.wake_pending().await.unwrap();

		for (work, thread, before) in histories {
			assert_eq!(timeline(&reopened.client, &thread, &reopened_store, &work).await, before);
		}

		assert_eq!(requests.load(Ordering::Acquire), 6, "restart and reads must not replay work");
	}))
	.catch_unwind()
	.await;

	drop(agent);
	drop(reopened);

	backend.abort();
	result.expect("native context restart panicked").expect("native context restart timed out");
}

async fn terminal(
	agent: &mut AgentCoordinator,
	events: &mut Receiver<ServerEvent>,
	store: &SqliteStore,
) {
	loop {
		let event = events.recv().await.expect("native event stream");

		assert!(
			!matches!(&event, ServerEvent::Notification { method, .. } if method == "rawResponseItem/completed")
		);

		let done = matches!(&event, ServerEvent::Notification { method, .. } if method == "turn/completed");

		agent.handle_event(event).await.expect("coordinator accepts native event");

		if done
			&& store
				.list_agent_work_items()
				.await
				.expect("fixture work state")
				.iter()
				.all(|work| work.dispatch_state == AgentDispatchState::Idle)
		{
			return;
		}
	}
}

async fn timeline(
	client: &AppServerClient,
	thread: &str,
	store: &SqliteStore,
	work: &str,
) -> Vec<Value> {
	let page = client.thread_timeline_page(thread, None, 100).await.expect("native timeline");

	assert!(page["nextCursor"].is_null(), "fixture must fit one complete page");

	let mut projected =
		crate::agent::timeline::project(thread, &page).expect("public timeline projection");

	metrics::enrich(store, work, &mut projected).await.expect("persisted native response usage");

	for entry in &projected.entries {
		if let AgentTimelineContent::TurnBoundary { completed: true, usage_summary, .. } =
			&entry.content
		{
			assert!(
				usage_summary
					.as_deref()
					.is_some_and(|text| text.contains("0.12345678901234567890")),
				"native response amount survives projection and reopen"
			);
		}
		if let AgentTimelineContent::Item { kind, text, .. } = &entry.content
			&& kind == "functionCallOutput"
		{
			assert!(
				text.starts_with("decodex/work_"),
				"tool history must remain visible with provenance"
			);
		}
	}

	page["data"].as_array().expect("timeline items").clone()
}

async fn checked_histories(
	client: &AppServerClient,
	store: &SqliteStore,
) -> Vec<(String, String, Vec<Value>)> {
	let mut histories = Vec::new();

	for work in ["agent", "worker"] {
		let thread = store
			.get_agent_work_item(work.into())
			.await
			.expect("read fixture work")
			.codex_thread_id
			.expect("fixture native thread");
		let items = timeline(client, &thread, store, work).await;

		assert_tool_authority(work, &items);

		histories.push((work.to_owned(), thread, items));
	}

	histories
}
