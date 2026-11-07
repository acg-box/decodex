//! Qualify tool-owned context through the installed native process and retained bridge.
use std::{
	env, fs,
	panic::AssertUnwindSafe,
	sync::{Mutex, atomic::AtomicUsize},
};

use futures_util::FutureExt as _;
use tokio::{net::TcpListener, sync::mpsc::Receiver, time};

use crate::{
	account_launch::agent_process::native_tests::{
		self, AppServerClient, Arc, Duration, NativeSession, Ordering, ServerEvent, Value,
	},
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
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native stream recovery"]
async fn installed_native_stream_disconnect_recovers_the_same_turn_without_resending() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(native_tests::serve_fixture_frames(
		listener,
		requests.clone(),
		None,
		Some(bodies.clone()),
		|serial| {
			let mut frames = vec![
				serde_json::json!({"type":"response.created","response":{"id":format!("reconnect-{serial}")}}),
			];
			// Close the first SSE response before its terminal event.
			if serial > 0 {
				frames.extend([
					serde_json::json!({"type":"response.output_item.done","item":{"type":"message","role":"assistant","id":"recovered-answer","content":[{"type":"output_text","text":"Connection recovered."}]}}),
					serde_json::json!({"type":"response.completed","response":{"id":format!("reconnect-{serial}"),"usage":{"input_tokens":100,"output_tokens":3,"total_tokens":103}}}),
				]);
			}
			frames
		},
	));
	fs::write(home.path().join("config.toml"), format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\ncli_auth_credentials_store=\"file\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"Local stream recovery\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\nstream_max_retries=2\nrequest_max_retries=0\n")).unwrap();
	let root = DecodexRoot::new(home.path().canonicalize().unwrap().join("state")).unwrap();

	root.paths().ensure_layout().unwrap();

	let store = SqliteStore::open(&root.paths()).unwrap();
	let mut session = NativeSession::start(&binary, home.path());
	let config =
		AgentConfig::new("gpt-5.6-sol".into(), "low".into(), home.path().display().to_string());
	let mut agent = AgentCoordinator::new(store.clone(), session.client.clone(), config).unwrap();
	let result = AssertUnwindSafe(time::timeout(Duration::from_secs(45), async {
		agent.start_agent("agent", "Reply once after the local connection recovers.").await.unwrap();
		let started = store.get_agent_work_item("agent".into()).await.unwrap();
		let mut retries = 0;

		assert!(started.codex_thread_id.is_some() && started.active_turn_id.is_some());

		loop {
			let event = session.events.recv().await.expect("native recovery event");
			let retry = matches!(&event, ServerEvent::Notification { method, params } if method == "error" && params["willRetry"] == true);
			let done = matches!(&event, ServerEvent::Notification { method, .. } if method == "turn/completed");

			agent.handle_event(event).await.unwrap();

			if retry {
				retries += 1;
				let pending = store.get_agent_work_item("agent".into()).await.unwrap();

				assert_eq!(pending.dispatch_state, AgentDispatchState::Running);
				assert_eq!(pending.active_turn_id, started.active_turn_id);
			}
			if done { break; }
		}
		let completed = store.get_agent_work_item("agent".into()).await.unwrap();

		assert_eq!(retries, 1);
		assert_eq!(completed.codex_thread_id, started.codex_thread_id);
		assert_eq!(completed.dispatch_state, AgentDispatchState::Idle);
		assert!(completed.active_turn_id.is_none());
		assert!(!store.list_agent_wake_events("agent".into(), 100).await.unwrap().iter().any(|event| event.event_kind == "reconnection_needs_attention"));

		agent.wake_pending().await.unwrap();

		assert_eq!(requests.load(Ordering::Acquire), 2);
		let bodies = bodies.lock().unwrap();
		let metadata: Vec<Value> = bodies.iter().map(|body| serde_json::from_str(body["client_metadata"]["x-codex-turn-metadata"].as_str().unwrap()).unwrap()).collect();

		assert_eq!(metadata[0]["thread_id"].as_str(), started.codex_thread_id.as_deref());
		assert_eq!(metadata[0]["turn_id"].as_str(), started.active_turn_id.as_deref());
		assert_eq!(metadata[0]["thread_id"], metadata[1]["thread_id"]);
		assert_eq!(metadata[0]["turn_id"], metadata[1]["turn_id"]);
	})).catch_unwind().await;

	drop(agent);
	drop(session);
	backend.abort();
	result.expect("native stream recovery panicked").expect("native stream recovery timed out");
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
	let backend = tokio::spawn(native_tests::serve_fixture(
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

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated Assistant lifecycle qualification"]
async fn installed_assistant_renews_fresh_contexts_with_original_dialogue_and_isolated_memory() {
	use decodex_core::assistant::{AssistantCommand, AssistantSettings, RagflowConnection};
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		requests.clone(),
		None,
		Some(bodies.clone()),
		None,
		|serial| serde_json::json!({"type":"message","role":"assistant","id":format!("assistant-answer-{serial}"),"content":[{"type":"output_text","text":"Recorded original user correction."}]}),
	));
	fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\ncli_auth_credentials_store=\"file\"\n[features]\nmemories=true\nenable_request_compression=false\n[memories]\ngenerate_memories=false\n[model_providers.fixture]\nname=\"Assistant fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).unwrap();
	fs::create_dir(home.path().join("memories_v2")).unwrap();
	fs::write(
		home.path().join("memories_v2/memory_summary.md"),
		"v1\nNATIVE_MEMORY_FIXTURE_SENTINEL\n",
	)
	.unwrap();
	let root = DecodexRoot::new(home.path().canonicalize().unwrap().join("state")).unwrap();
	root.paths().ensure_layout().unwrap();
	let store = SqliteStore::open(&root.paths()).unwrap();
	let mut session = NativeSession::start(&binary, home.path());
	let mut config =
		AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), home.path().display().to_string());
	config.sandbox = "read-only".into();
	config.approval_policy = serde_json::json!("never");
	let mut agent = AgentCoordinator::new(store.clone(), session.client.clone(), config).unwrap();
	let result = AssertUnwindSafe(time::timeout(Duration::from_secs(90), async {
		agent
			.start_agent("assistant", "Original user decision: inspect sources; do not publish.")
			.await
			.unwrap();
		terminal(&mut agent, &mut session.events, &store).await;
		let original =
			store.get_agent_work_item("assistant".into()).await.unwrap().codex_thread_id.unwrap();
		assert!(
			session
				.client
				.memory_summary(&original)
				.await
				.unwrap()
				.unwrap()
				.contains("NATIVE_MEMORY_FIXTURE_SENTINEL")
		);
		store.set_assistant("assistant".into(), true).await.unwrap();
		agent
			.manage_assistant(
				"assistant",
				"settings",
				AssistantCommand::Configure {
					settings: AssistantSettings {
						mandate: "Inspect sources; do not publish.".into(),
						renew_after_turns: 1,
						ragflow: Some(RagflowConnection {
							url: "https://ragflow.invalid".into(),
							credential_id: "a".repeat(64),
							user_id: "fixture-user".into(),
							dataset_ids: vec![],
							memory_id: Some("fixture-memory".into()),
							ingest_chats: false,
						}),
					},
				},
				true,
			)
			.await
			.unwrap();
		let mut seen = std::collections::BTreeSet::from([original.clone()]);
		for n in 0..3 {
			agent
				.enqueue_user_message(
					"assistant",
					&format!("correction-{n}"),
					&format!("Original correction {n}: use blue and keep publication disabled."),
				)
				.await
				.unwrap();
			agent.wake_pending().await.unwrap();
			terminal(&mut agent, &mut session.events, &store).await;
			let current = store.get_agent_work_item("assistant".into()).await.unwrap();
			let thread = current.codex_thread_id.unwrap();
			assert!(seen.insert(thread.clone()), "each renewal starts a distinct native context");
			assert_eq!(session.client.memory_summary(&thread).await.unwrap(), None);
			let metadata =
				session.client.thread_read(serde_json::json!({"threadId":thread})).await.unwrap();
			assert!(metadata["thread"]["forkedFromId"].is_null());
			assert!(metadata["thread"]["parentThreadId"].is_null());
            let owned=bodies.lock().unwrap().iter().filter(|b|b["client_metadata"]["thread_id"].as_str().is_some_and(|id|seen.contains(id))).count();
            assert_eq!(owned,n+2,"one model request per user turn; renewal makes no additional inference call");
		}
		assert_eq!(store.agent_previous_threads("assistant".into()).await.unwrap().len(), 3);
        let captured=bodies.lock().unwrap().clone();
        let observed=captured.iter().filter(|b|b["client_metadata"]["thread_id"].as_str().is_some_and(|id|seen.contains(id))).collect::<Vec<_>>();
		assert!(!observed[0].to_string().contains("assistant_memory"), "ordinary Agents do not expose personal Assistant tools");
		for body in &observed[1..] {
			assert!(body.to_string().contains("assistant_memory"), "selected Assistant contexts retain their personal memory instructions");
		}
		assert!(observed[1].to_string().contains("Personal memory could not be recalled"), "automatic recall is supplied as untrusted native context even when the optional service is unavailable");
		assert!(observed[1].to_string().contains("Original user decision"));
		assert!(observed[3].to_string().contains("Original correction 1"));
        assert!(observed[3].to_string().contains("Original correction 0"));
		for body in &observed[1..] {
			assert!(!body.to_string().contains("NATIVE_MEMORY_FIXTURE_SENTINEL"));
			assert!(body.to_string().contains("do not publish"));
		}
		let control=session.client.thread_start(serde_json::json!({"cwd":home.path(),"ephemeral":true,"approvalPolicy":"never","sandbox":"read-only","config":{"memories.generate_memories":false}})).await.unwrap();
        assert!(session.client.memory_summary(control["thread"]["id"].as_str().unwrap()).await.unwrap().unwrap().contains("NATIVE_MEMORY_FIXTURE_SENTINEL"),"other contexts keep native memory settings");
	}))
	.catch_unwind()
	.await;
	drop(agent);
	drop(session);
	backend.abort();
	result
		.expect("Assistant native qualification panicked")
		.expect("Assistant native qualification timed out");
}
