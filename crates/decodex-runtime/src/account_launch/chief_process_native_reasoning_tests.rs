//! A synthetic Responses summary crosses native history and the retained coordinator.
use super::*;
use crate::chief::{ChiefConfig, ChiefCoordinator};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated public reasoning history"]
async fn installed_public_reasoning_is_saved_and_projected_after_cold_restart() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").unwrap();
	let home = tempfile::tempdir().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let backend = tokio::spawn(serve_fixture(listener, calls.clone(), None, None, None, |_| {
		json!([
			{"type":"reasoning","id":"reasoning-fixture","summary":[{"type":"summary_text","text":"Public summary fixture."}],"content":[{"type":"reasoning_text","text":"PRIVATE_RAW_FIXTURE"}]},
			{"type":"message","id":"answer-fixture","role":"assistant","content":[{"type":"output_text","text":"Done."}]}
		])
	}));
	std::fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).unwrap();
	let root =
		decodex_core::DecodexRoot::new(home.path().canonicalize().unwrap().join("state")).unwrap();
	root.paths().ensure_layout().unwrap();
	let store = decodex_database::SqliteStore::open(&root.paths()).unwrap();
	let mut session = NativeSession::start(&binary, home.path());
	let mut config =
		ChiefConfig::new("gpt-5.6-sol".into(), "high".into(), home.path().display().to_string());
	config.approval_policy = json!("never");
	config.sandbox = "read-only".into();
	let mut chief = ChiefCoordinator::new(store.clone(), session.client.clone(), config).unwrap();
	let work = chief.start_chief("chief", "Summarize the fixture").await.unwrap();
	let thread = work.codex_thread_id.unwrap();
	let mut observed = false;
	tokio::time::timeout(Duration::from_secs(30), async {
		loop {
			let event = session.events.recv().await.unwrap();
			let done =
				matches!(&event,ServerEvent::Notification{method,..} if method=="turn/completed");
			chief.handle_event(event).await.unwrap();
			for row in store.read_chief_output("chief".into()).await.unwrap() {
				if row.kind == "reasoningSummary" {
					assert!(!row.text.contains("PRIVATE"));
					observed |= row.text == "Public summary fixture.";
				}
			}
			if done {
				break;
			}
		}
	})
	.await
	.unwrap();
	assert!(observed);
	let before = read(&store, &session.client, &thread).await;
	assert!(before.contains("Public summary fixture."));
	assert!(!before.contains("PRIVATE_RAW_FIXTURE"));
	drop(chief);
	drop(session);
	drop(store);
	let store = decodex_database::SqliteStore::open(&root.paths()).unwrap();
	let session = NativeSession::start(&binary, home.path());
	assert_eq!(read(&store, &session.client, &thread).await, before);
	assert_eq!(calls.load(Ordering::Acquire), 1);
	backend.abort();
}

async fn read(
	store: &decodex_database::SqliteStore,
	client: &AppServerClient,
	thread: &str,
) -> String {
	let result = crate::chief::timeline::read(
		Some(store),
		|| {
			std::future::ready(Some(crate::chief_usage_estimate::Source {
				client: client.clone(),
				key: crate::chief_usage_estimate::SourceKey {
					work: "chief".into(),
					thread: thread.into(),
					revision: 1,
					history_revision: client.history_revision(),
					account: decodex_core::AccountId::new("10000000-0000-4000-8000-000000000001")
						.expect("native reasoning fixture"),
					generation: decodex_core::ProcessGenerationId::new(
						"20000000-0000-4000-8000-000000000002",
					)
					.expect("native reasoning fixture"),
				},
			}))
		},
		None,
	)
	.await;
	let decodex_protocol::ChiefTimelineResult::Available { page, .. } = result else {
		panic!("native summary projection unavailable")
	};
	serde_json::to_string(&page).expect("native reasoning fixture")
}
