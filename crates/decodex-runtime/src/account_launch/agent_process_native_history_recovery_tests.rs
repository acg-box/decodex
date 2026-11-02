//! Cold fork history uses native ancestor-copy recovery without client file substitution.
use std::{env, fs, path::PathBuf, sync::atomic::AtomicUsize};

use tokio::net::TcpListener;

use crate::account_launch::agent_process::native_tests::{
	self, Arc, Duration, NativeSession, Ordering, ServerEvent, time,
};
use decodex_codex::app_server_client::ThreadForkBoundary;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated inherited history recovery"]
async fn installed_native_fork_history_recovers_from_intact_ancestor_copy() {
	for attached in [false, true] {
		time::timeout(Duration::from_secs(45), qualify(attached))
			.await
			.expect("bounded ancestor recovery fixture");
	}
}

async fn qualify(attached: bool) {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir_in("/tmp").expect("isolated history home");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("history fixture listener");
	let address = listener.local_addr().expect("history fixture address");
	let requests = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		requests.clone(),
		None,
		Some(bodies.clone()),
		None,
		|serial| serde_json::json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Inherited fixture answer"}]}),
	));
	fs::write(home.path().join("config.toml"), format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"History fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("history fixture configuration");
	let mut session = NativeSession::start(&binary, home.path());
	let parent = session.client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only"})).await.expect("history source")["thread"]["id"].as_str().expect("source identity").to_owned();
	let turn = session.client.turn_start(serde_json::json!({"threadId":parent,"input":[{"type":"text","text":"RETAINED_ANCESTOR_CONTEXT"}]})).await.expect("source inference")["turn"]["id"].as_str().expect("source turn").to_owned();
	wait_for_completion(&mut session, &parent).await;
	let path = PathBuf::from(
		session
			.client
			.thread_read(serde_json::json!({"threadId":parent}))
			.await
			.expect("source metadata")["thread"]["path"]
			.as_str()
			.expect("source rollout path"),
	);
	let fork = session
		.client
		.fork_thread_at_boundary(
			&parent,
			if attached {
				ThreadForkBoundary::AttachedAfterTurn(&turn)
			} else {
				ThreadForkBoundary::AfterTurn(&turn)
			},
			session.client.thread_settings_guard(&parent).expect("source settings guard"),
		)
		.await
		.expect("native fork")["thread"]["id"]
		.as_str()
		.expect("fork identity")
		.to_owned();
	let before =
		session.client.thread_history_page(&fork, None, 1).await.expect("intact inherited history");
	assert_eq!(before["turns"].as_array().expect("fork turns").len(), usize::from(!attached));
	drop(session);

	// The fixture owns this home. Preserve the exact native ancestor in its managed archive,
	// then leave the preferred active copy readable but shorter than the frozen child prefix.
	let contents = fs::read(&path).expect("ancestor bytes");
	let archive = home.path().join("archived_sessions");
	fs::create_dir_all(&archive).expect("archive fixture directory");
	fs::copy(&path, archive.join(path.file_name().expect("ancestor filename")))
		.expect("intact ancestor copy");
	let header_end = contents.iter().position(|byte| *byte == b'\n').expect("ancestor header") + 1;
	fs::write(&path, &contents[..header_end]).expect("short preferred ancestor");

	let mut reopened = NativeSession::start(&binary, home.path());
	reopened
		.client
		.thread_resume(serde_json::json!({"threadId":fork,"excludeTurns":true}))
		.await
		.unwrap_or_else(|error| match error {
			decodex_codex::app_server_client::ClientError::Remote(error) => {
				panic!("cold resume through native ancestor recovery: {}", error.message)
			},
			error => panic!("cold resume through native ancestor recovery: {error:?}"),
		});
	let after = reopened
		.client
		.thread_history_page(&fork, None, 1)
		.await
		.expect("cold inherited history page");
	assert_eq!(
		after["turns"], before["turns"],
		"cold reopen must retain the ordinary prefix and the attached fork's empty visible history"
	);
	assert_eq!(requests.load(Ordering::Acquire), 1, "history and resume must not infer");
	reopened
		.client
		.turn_start(
			serde_json::json!({"threadId":fork,"input":[{"type":"text","text":"Continue recovered child"}]}),
		)
		.await
		.expect("explicit child inference");
	wait_for_completion(&mut reopened, &fork).await;
	assert_eq!(
		requests.load(Ordering::Acquire),
		2,
		"only the explicit child continuation may infer"
	);
	let input = bodies
		.lock()
		.expect("captured native requests")
		.last()
		.expect("continued child request")["input"]
		.to_string();
	assert!(
		input.contains("RETAINED_ANCESTOR_CONTEXT"),
		"the model must receive the recovered ancestor"
	);
	assert!(input.contains("Continue recovered child"));
	assert_eq!(
		fs::read(&path).expect("unchanged short ancestor"),
		contents[..header_end],
		"the consumer must not replace the preferred rollout"
	);
	backend.abort();
}

async fn wait_for_completion(session: &mut NativeSession, thread: &str) {
	loop {
		if let ServerEvent::Notification { method, params } =
			session.events.recv().await.expect("native history event")
			&& method == "turn/completed"
			&& params["threadId"] == thread
		{
			assert_eq!(params["turn"]["status"], "completed");
			break;
		}
	}
}
