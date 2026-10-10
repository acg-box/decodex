//! Cross-version native persistence qualification using isolated fixture state.
use std::{env, fs, sync::atomic::AtomicUsize};

use tokio::{net::TcpListener, time};

use super::{Arc, Duration, NativeSession, Ordering, ServerEvent, serve};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_PREVIOUS_CODEX_BINARY and DECODEX_TEST_CODEX_BINARY"]
async fn installed_runtime_upgrade_preserves_history_paused_goal_and_voice() {
	time::timeout(Duration::from_secs(45), qualify()).await.expect("native upgrade fixture");
}

async fn qualify() {
	let previous =
		env::var_os("DECODEX_TEST_PREVIOUS_CODEX_BINARY").expect("native upgrade fixture");
	let candidate = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native upgrade fixture");
	assert_ne!(previous, candidate, "Upgrade qualification needs two runtime artifacts");
	let home = tempfile::tempdir().expect("native upgrade fixture");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("native upgrade fixture");
	let address = listener.local_addr().expect("native upgrade fixture");
	let requests = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(listener, requests.clone()));
	fs::write(home.path().join("config.toml"), format!(
        "model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\ncli_auth_credentials_store=\"file\"\n[features]\ngoals=true\n[realtime]\nvoice=\"juniper\"\n[model_providers.fixture]\nname=\"Upgrade fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n"
    )).expect("native upgrade fixture");

	let mut old = NativeSession::start(&previous, home.path());
	let started = old
		.client
		.thread_start(
			serde_json::json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only"}),
		)
		.await
		.expect("native upgrade fixture");
	let thread = started["thread"]["id"].as_str().expect("native upgrade fixture").to_owned();
	old.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Keep this history across the runtime upgrade"}]})).await.expect("native upgrade fixture");
	loop {
		if let ServerEvent::Notification { method, params } =
			old.events.recv().await.expect("native upgrade fixture")
			&& method == "turn/completed"
		{
			assert_eq!(params["turn"]["status"], "completed");
			break;
		}
	}
	old.client.request("thread/goal/set", serde_json::json!({"threadId":thread,"objective":"Keep the paused goal","status":"paused","tokenBudget":12345})).await.expect("native upgrade fixture");
	let goal = old.client.thread_goal(&thread).await.expect("native upgrade fixture");
	let before = old
		.client
		.thread_read(serde_json::json!({"threadId":thread,"includeTurns":true}))
		.await
		.expect("native upgrade fixture");
	assert!(before["thread"]["turns"].to_string().contains("Native bridge answer"));
	drop(old);

	let current = NativeSession::start(&candidate, home.path());
	current
		.client
		.thread_resume(serde_json::json!({"threadId":thread}))
		.await
		.expect("native upgrade fixture");
	let after = current
		.client
		.thread_read(serde_json::json!({"threadId":thread,"includeTurns":true}))
		.await
		.expect("native upgrade fixture");
	assert_eq!(after["thread"]["turns"], before["thread"]["turns"]);
	assert_eq!(
		serde_json::to_value(
			current.client.thread_goal(&thread).await.expect("native upgrade fixture")
		)
		.expect("native upgrade fixture"),
		serde_json::to_value(goal).expect("native upgrade fixture")
	);
	let voice = current
		.client
		.realtime_voice_settings(home.path().to_str().expect("native upgrade fixture"))
		.await
		.expect("native upgrade fixture");
	assert_eq!(voice.preference.as_deref(), Some("juniper"));
	assert_eq!(voice.effective.as_deref(), Some("juniper"));
	assert_eq!(requests.load(Ordering::Acquire), 1, "Migration must not replay model work");
	drop(current);
	backend.abort();
}
