//! Qualify restored native token counters through the real Chief and SQLite owners.
use super::*;
use crate::chief::{ChiefConfig, ChiefCoordinator};
use decodex_database::SqliteStore;
use std::sync::atomic::AtomicUsize;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native usage qualification"]
async fn installed_native_usage_restores_chief_baseline_after_cold_resume() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve_with_usage(listener, requests.clone(), None, |serial| {
		let count = serial + 1;
		json!({"input_tokens":10*count,"output_tokens":2*count,"total_tokens":12*count})
	}));
	std::fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[model_providers.fixture]\nname = \"Isolated usage fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();
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
	let first = tokio::time::timeout(Duration::from_secs(30), async {
		chief.start_chief("chief", "Complete the first usage fixture turn.").await.unwrap();
		let (turn, raw) = finish(&mut chief, &mut session.events).await;
		assert_eq!(raw, 1, "new native thread must deliver its opted-in raw response");
		assert_usage(&store, 10, 2).await;
		turn
	})
	.await
	.unwrap();
	let thread = store.get_chief_work_item("chief".into()).await.unwrap().codex_thread_id.unwrap();
	drop(chief);
	drop(session);
	drop(store);
	let store = SqliteStore::open(&root.paths()).unwrap();
	// A disconnected reader can have no trustworthy local baseline.
	store.validate_chief_usage_resume(thread.clone(), None).await.unwrap();
	assert!(store.read_chief_usage("chief".into()).await.unwrap().is_none());
	let mut session = NativeSession::start(&binary, home.path());
	let mut chief = ChiefCoordinator::new(store.clone(), session.client.clone(), config).unwrap();
	tokio::time::timeout(Duration::from_secs(30), async {
		chief.recover_persisted().await.unwrap();
		assert_eq!(requests.load(Ordering::Acquire), 1, "idle recovery must not replay input");
		chief.continue_worker("chief", "Complete the second usage fixture turn.").await.unwrap();
		let (second, raw) = finish(&mut chief, &mut session.events).await;
		assert_ne!(first, second);
		assert_eq!(raw, 0, "installed native cold resume does not enable raw response events");
		assert_usage(&store, 30, 6).await;
		let metrics = store
			.read_chief_turn_metrics("chief".into(), thread.clone(), vec![second.clone()])
			.await
			.unwrap();
		let usage: Value = serde_json::from_str(
			metrics[0].usage_json.as_ref().expect("complete second-turn delta"),
		)
		.unwrap();
		assert_eq!(usage["input_tokens"], 20, "restored baseline must exclude the first turn");
		assert_eq!(usage["output_tokens"], 4);
		let responses = store
			.read_chief_response_usage(
				"chief".into(),
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
	drop(chief);
	drop(session);
	backend.abort();
}

async fn assert_usage(store: &SqliteStore, input: u64, output: u64) {
	let usage: Value = serde_json::from_str(
		&store
			.read_chief_usage("chief".into())
			.await
			.expect("native usage fixture operation")
			.expect("native usage fixture operation"),
	)
	.expect("native usage fixture operation");
	assert_eq!(usage["input_tokens"], input);
	assert_eq!(usage["output_tokens"], output);
}

async fn finish(
	chief: &mut ChiefCoordinator,
	events: &mut mpsc::Receiver<ServerEvent>,
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
		chief.handle_event(event).await.expect("native usage fixture operation");
		if let Some(turn) = done {
			return (turn, raw);
		}
	}
}
