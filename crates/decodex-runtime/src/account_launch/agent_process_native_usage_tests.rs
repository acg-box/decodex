//! Qualify restored native token counters through the real Agent and SQLite owners.
use std::{env, fs, sync::atomic::AtomicUsize};

use mpsc::Receiver;
use tokio::{net::TcpListener, time};

use crate::{
	account_launch::agent_process::native_tests::*,
	agent::{AgentConfig, AgentCoordinator},
};
use decodex_core::DecodexRoot;
use decodex_database::SqliteStore;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native usage qualification"]
async fn installed_native_usage_restores_agent_baseline_after_cold_resume() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");

	assert!(std::path::Path::new(&binary).is_absolute());

	let home = tempfile::tempdir().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve_fixture_usage(
		listener,
		requests.clone(),
		None,
		None,
		|serial| {
			let count = serial + 1;

			serde_json::json!({"input_tokens":10*count,"output_tokens":2*count,"total_tokens":12*count})
		},
		|serial| serde_json::json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Native usage answer"}]}),
	));

	fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[model_providers.fixture]\nname = \"Isolated usage fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();

	let root = DecodexRoot::new(home.path().canonicalize().unwrap().join("state")).unwrap();

	root.paths().ensure_layout().unwrap();

	let store = SqliteStore::open(&root.paths()).unwrap();
	let mut config =
		AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), home.path().display().to_string());

	config.sandbox = "read-only".into();
	config.approval_policy = serde_json::json!("never");

	let mut session = NativeSession::start(&binary, home.path());
	let mut agent =
		AgentCoordinator::new(store.clone(), session.client.clone(), config.clone()).unwrap();
	let first = time::timeout(Duration::from_secs(30), async {
		agent.start_agent("agent", "Complete the first usage fixture turn.").await.unwrap();

		let (turn, raw) = finish(&mut agent, &mut session.events).await;

		assert_eq!(raw, 1, "new native thread must deliver its opted-in raw response");

		assert_usage(&store, 10, 2).await;

		turn
	})
	.await
	.unwrap();
	let thread = store.get_agent_work_item("agent".into()).await.unwrap().codex_thread_id.unwrap();

	drop(agent);
	drop(session);
	drop(store);

	let store = SqliteStore::open(&root.paths()).unwrap();
	// A disconnected reader can have no trustworthy local baseline.
	store.validate_agent_usage_resume(thread.clone(), None).await.unwrap();

	assert!(store.read_agent_usage("agent".into()).await.unwrap().is_none());

	let mut session = NativeSession::start(&binary, home.path());
	let mut agent = AgentCoordinator::new(store.clone(), session.client.clone(), config).unwrap();

	time::timeout(Duration::from_secs(30), async {
		agent.recover_persisted().await.unwrap();

		assert_eq!(requests.load(Ordering::Acquire), 1, "idle recovery must not replay input");

		agent.continue_worker("agent", "Complete the second usage fixture turn.").await.unwrap();

		let (second, raw) = finish(&mut agent, &mut session.events).await;

		assert_ne!(first, second);
		assert_eq!(raw, 0, "installed native cold resume does not enable raw response events");

		assert_usage(&store, 30, 6).await;

		let metrics = store
			.read_agent_turn_metrics("agent".into(), thread.clone(), vec![second.clone()])
			.await
			.unwrap();
		let usage: Value = serde_json::from_str(
			metrics[0].usage_json.as_ref().expect("complete second-turn delta"),
		)
		.unwrap();

		assert_eq!(usage["input_tokens"], 20, "restored baseline must exclude the first turn");
		assert_eq!(usage["output_tokens"], 4);

		let responses = store
			.read_agent_response_usage(
				"agent".into(),
				thread.clone(),
				vec![first.clone(), second.clone()],
			)
			.await
			.unwrap();

		assert!(responses.iter().any(|row| row.turn_id == first));
		assert!(
			!responses.iter().any(|row| row.turn_id == second),
			"missing response amounts must not be synthesized"
		);
		assert_eq!(requests.load(Ordering::Acquire), 2);
	})
	.await
	.unwrap();

	drop(agent);
	drop(session);

	backend.abort();
}

async fn assert_usage(store: &SqliteStore, input: u64, output: u64) {
	let usage: Value = serde_json::from_str(
		&store
			.read_agent_usage("agent".into())
			.await
			.expect("native usage fixture operation")
			.expect("native usage fixture operation"),
	)
	.expect("native usage fixture operation");

	assert_eq!(usage["input_tokens"], input);
	assert_eq!(usage["output_tokens"], output);
}

async fn finish(
	agent: &mut AgentCoordinator,
	events: &mut Receiver<ServerEvent>,
) -> (String, usize) {
	let mut raw = 0;

	loop {
		let event = events.recv().await.expect("native usage fixture event");
		let done = if let ServerEvent::Notification { method, params } = &event {
			raw += usize::from(method == "rawResponse/completed");

			(method == "turn/completed").then(|| {
				assert_eq!(params["turn"]["status"], "completed");

				params["turn"]["id"].as_str().expect("native usage fixture operation").to_owned()
			})
		} else {
			None
		};

		agent.handle_event(event).await.expect("native usage fixture operation");

		if let Some(turn) = done {
			return (turn, raw);
		}
	}
}
