//! Native warning transport, current owner persistence and user-visible history.
use std::{
	env, fs,
	path::{Path, PathBuf},
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
	time::Duration,
};

use tokio::{net::TcpListener, time};

use crate::{
	account_launch::agent_process::{
		native_tests,
		native_tests::reviewer::{
			NativeSession,
			store::{GENERATION, OwnedReviewer, ProcessGenerationId, SqliteStore},
		},
	},
	application, native_config_warning,
};
use decodex_codex::app_server_client::ServerEvent;

fn prepare_warning_fixture(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
	let home = root.join("home");
	let project = root.join("project");

	fs::create_dir(&home).expect("Create warning fixture home");
	fs::create_dir(&project).expect("Create warning fixture project");

	let home = home.canonicalize().expect("Resolve warning fixture home");
	let project = project.canonicalize().expect("Resolve warning fixture project");
	let source = home.join("AGENTS.md");

	fs::write(&source, "Keep the fixture local.").expect("Write warning fixture instructions");

	(home, project, source)
}

async fn record_turn_warnings(session: &mut NativeSession, owned: &OwnedReviewer, thread: &str) {
	let generation = ProcessGenerationId::new(GENERATION).expect("warning generation");
	let mut seen = 0;

	loop {
		let event = session.events.recv().await.expect("native warning event");

		if let ServerEvent::Notification { method, params } = &event {
			native_config_warning::record_notification(
				&owned.store,
				"root",
				&generation,
				method,
				params,
			)
			.await
			.expect("record native warning");

			if method == "warning" {
				// Exercise the alternate startup notification shape with the real
				// native message through the same production owner.
				native_config_warning::record_notification(
					&owned.store,
					"root",
					&generation,
					"configWarning",
					&serde_json::json!({"summary":params["message"],"details":null}),
				)
				.await
				.expect("record native warning");

				assert_eq!(params["threadId"], thread);
				assert!(params["message"].as_str().expect("warning message").contains("AGENTS.md"));

				seen += 1;

				native_config_warning::record_notification(
					&owned.store,
					"root",
					&generation,
					method,
					params,
				)
				.await
				.expect("record native warning");
			}
			if method == "turn/completed" {
				assert_eq!(params["turn"]["status"], "completed");

				break;
			}
		}
	}

	assert_eq!(seen, 1);
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native global instruction failure"]
async fn installed_native_warning_crosses_bridge_owner_history_and_reopen() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let temporary = tempfile::tempdir().unwrap();
	let (home, project, source) = prepare_warning_fixture(temporary.path());
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		calls.clone(),
		None,
		None,
		Some(serde_json::json!({"input_tokens":1,"output_tokens":1,"total_tokens":2})),
		|n| serde_json::json!({"type":"message","role":"assistant","id":format!("done-{n}"),"content":[{"type":"output_text","text":"Done"}]}),
	));

	fs::write(home.join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\nstep_model_switching=false\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).unwrap();

	time::timeout(Duration::from_secs(45), async {
		let mut session = NativeSession::start(&binary, &home);
		let thread = session
			.client
			.thread_start(
				serde_json::json!({"cwd":project,"approvalPolicy":"never","sandbox":"read-only"}),
			)
			.await
			.unwrap()["thread"]["id"]
			.as_str()
			.unwrap()
			.to_owned();

		session
			.client
			.turn_start(
				serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Say Done"}]}),
			)
			.await
			.unwrap();

		super::super::finish(&mut session.events).await;

		let owned = OwnedReviewer::new(temporary.path(), &session.client, &thread, "initial").await;

		owned.store.complete_agent_turn("root".into(), "initial".into()).await.unwrap();

		fs::remove_file(&source).unwrap();
		std::os::unix::fs::symlink("AGENTS.md", &source).unwrap();

		owned.store.begin_agent_dispatch("root".into()).await.unwrap();

		let started = session
			.client
			.turn_start(
				serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Continue"}]}),
			)
			.await
			.unwrap();
		let turn = started["turn"]["id"].as_str().unwrap().to_owned();

		owned.store.acknowledge_agent_dispatch("root".into(), turn.clone()).await.unwrap();

		record_turn_warnings(&mut session, &owned, &thread).await;

		assert_eq!(
			owned.store.get_agent_work_item("root".into()).await.unwrap().active_turn_id,
			Some(turn.clone()),
			"a notice must not terminate work"
		);

		owned.store.complete_agent_turn("root".into(), turn).await.unwrap();

		let reopened = SqliteStore::open(&owned.root.paths()).unwrap();
		let events = reopened.read_agent_transcript("root".into(), None, 32).await.unwrap().0;
		let notices: Vec<_> = events
			.into_iter()
			.filter(|event| {
				matches!(event.event_kind.as_str(), "native_warning" | "config_warning")
			})
			.collect();

		assert_eq!(notices.len(), 1);

		let rendered = application::render_agent_history_for_test(notices);

		assert_eq!(rendered[0].kind, "execution_notice");
		assert!(rendered[0].text.contains("Failed to read global AGENTS.md"));
		assert!(reopened.list_agent_wake_events("root".into(), 32).await.unwrap().is_empty());
		assert_eq!(calls.load(Ordering::Acquire), 2);
	})
	.await
	.unwrap();

	backend.abort();
}
