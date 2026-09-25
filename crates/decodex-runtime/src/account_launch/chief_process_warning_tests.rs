//! Native warning transport, current owner persistence and user-visible history.
use super::*;
use decodex_codex::app_server_client::ServerEvent;
use serde_json::json;
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native global instruction failure"]
async fn installed_native_warning_crosses_bridge_owner_history_and_reopen() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let temporary = tempfile::tempdir().unwrap();
	let home = temporary.path().join("home");
	let project = temporary.path().join("project");
	std::fs::create_dir(&home).unwrap();
	std::fs::create_dir(&project).unwrap();
	let home = home.canonicalize().unwrap();
	let project = project.canonicalize().unwrap();
	let source = home.join("AGENTS.md");
	std::fs::write(&source, "Keep the fixture local.").unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(super::super::serve_fixture(
		listener,
		calls.clone(),
		None,
		None,
		Some(json!({"input_tokens":1,"output_tokens":1,"total_tokens":2})),
		|n| json!({"type":"message","role":"assistant","id":format!("done-{n}"),"content":[{"type":"output_text","text":"Done"}]}),
	));
	std::fs::write(home.join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\nstep_model_switching=false\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).unwrap();
	tokio::time::timeout(std::time::Duration::from_secs(45), async {
		let mut session = super::super::NativeSession::start(&binary, &home);
		let thread = session
			.client
			.thread_start(json!({"cwd":project,"approvalPolicy":"never","sandbox":"read-only"}))
			.await
			.unwrap()["thread"]["id"]
			.as_str()
			.unwrap()
			.to_owned();
		session
			.client
			.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Say Done"}]}))
			.await
			.unwrap();
		super::super::finish(&mut session.events).await;
		let owned = OwnedReviewer::new(temporary.path(), &session.client, &thread, "initial").await;
		owned.store.complete_chief_turn("root".into(), "initial".into()).await.unwrap();
		std::fs::remove_file(&source).unwrap();
		std::os::unix::fs::symlink("AGENTS.md", &source).unwrap();
		owned.store.begin_chief_dispatch("root".into()).await.unwrap();
		let started = session
			.client
			.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Continue"}]}))
			.await
			.unwrap();
		let turn = started["turn"]["id"].as_str().unwrap().to_owned();
		owned.store.acknowledge_chief_dispatch("root".into(), turn.clone()).await.unwrap();
		let generation = ProcessGenerationId::new(GENERATION).unwrap();
		let mut seen = 0;
		loop {
			let event = session.events.recv().await.unwrap();
			if let ServerEvent::Notification { method, params } = &event {
				crate::native_config_warning::record_notification(
					&owned.store,
					"root",
					&generation,
					method,
					params,
				)
				.await
				.unwrap();
				if method == "warning" {
					// Exercise the alternate startup notification shape with the real
					// native message through the same production owner.
					crate::native_config_warning::record_notification(
						&owned.store,
						"root",
						&generation,
						"configWarning",
						&json!({"summary":params["message"],"details":null}),
					)
					.await
					.unwrap();
					assert_eq!(params["threadId"], thread);
					assert!(params["message"].as_str().unwrap().contains("AGENTS.md"));
					seen += 1;
					crate::native_config_warning::record_notification(
						&owned.store,
						"root",
						&generation,
						method,
						params,
					)
					.await
					.unwrap();
				}
				if method == "turn/completed" {
					assert_eq!(params["turn"]["status"], "completed");
					break;
				}
			}
		}
		assert_eq!(seen, 1);
		assert_eq!(
			owned.store.get_chief_work_item("root".into()).await.unwrap().active_turn_id,
			Some(turn.clone()),
			"a notice must not terminate work"
		);
		owned.store.complete_chief_turn("root".into(), turn).await.unwrap();
		let reopened = SqliteStore::open(&owned.root.paths()).unwrap();
		let events = reopened.read_chief_transcript("root".into(), None, 32).await.unwrap().0;
		let notices: Vec<_> = events
			.into_iter()
			.filter(|event| {
				matches!(event.event_kind.as_str(), "native_warning" | "config_warning")
			})
			.collect();
		assert_eq!(notices.len(), 1);
		let rendered = crate::application::render_chief_history_for_test(notices);
		assert_eq!(rendered[0].kind, "execution_notice");
		assert!(rendered[0].text.contains("Failed to read global AGENTS.md"));
		assert!(reopened.list_chief_wake_events("root".into(), 32).await.unwrap().is_empty());
		assert_eq!(calls.load(Ordering::Acquire), 2);
	})
	.await
	.unwrap();
	backend.abort();
}
