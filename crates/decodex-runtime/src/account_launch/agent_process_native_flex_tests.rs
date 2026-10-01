//! Preserve explicit Flex through native settings and cold resume with fast mode disabled.
use std::{
	env, fs,
	sync::{Mutex, atomic::AtomicUsize},
};

use tokio::{net::TcpListener, time};

use crate::account_launch::agent_process::native_tests::*;
use decodex_codex::app_server_client::ThreadModelRecoveryUpdate;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated Flex request qualification"]
async fn installed_native_flex_survives_settings_and_cold_resume() {
	time::timeout(Duration::from_secs(30), qualify(false)).await.expect("Flex deadline");
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated configured Flex qualification"]
async fn installed_native_configured_flex_survives_cold_resume() {
	time::timeout(Duration::from_secs(30), qualify(true)).await.expect("Flex deadline");
}

async fn qualify(configured: bool) {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native binary");
	let home = tempfile::tempdir().expect("isolated home");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("fixture listener");
	let address = listener.local_addr().expect("fixture address");
	let count = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(serve_fixture_usage(
		listener,
		count.clone(),
		None,
		Some(bodies.clone()),
		|_| json!({"input_tokens":1,"output_tokens":1,"total_tokens":2}),
		|serial| json!({"type":"message","role":"assistant","id":format!("message-{serial}"),"content":[{"type":"output_text","text":"Done"}]}),
	));
	let tier = if configured { "service_tier=\"flex\"\n" } else { "" };

	fs::write(home.path().join("config.toml"),format!("{tier}model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nfast_mode=false\nenable_request_compression=false\n[model_providers.fixture]\nname=\"Fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("fixture config");

	let mut session = NativeSession::start(&binary, home.path());
	let started = session
		.client
		.thread_start(
			json!({"cwd":home.path(),"model":"gpt-5.6-sol","approvalPolicy":"never","sandbox":"read-only"}),
		)
		.await
		.expect("start");
	let thread = started["thread"]["id"].as_str().expect("thread").to_owned();

	if !configured {
		let update = ThreadModelRecoveryUpdate::new(&thread, "gpt-5.6-sol", "low", "flex")
			.expect("tier update");
		let guard = session
			.client
			.history_guard(session.client.history_revision())
			.expect("settings guard");

		session.client.queue_thread_model_recovery(&update, guard).await.expect("native update");

		loop {
			if let ServerEvent::Notification { method, params } =
				session.events.recv().await.expect("settings event")
				&& method == "thread/settings/updated"
				&& params["threadId"] == thread
			{
				assert_eq!(params["threadSettings"]["serviceTier"], "flex");

				break;
			}
		}
	}

	for cold in [false, true] {
		if cold {
			drop(session);

			session = NativeSession::start(&binary, home.path());

			let resumed = session
				.client
				.thread_resume(json!({"threadId":thread,"excludeTurns":true}))
				.await
				.expect("cold resume");

			assert_eq!(resumed["serviceTier"], "flex");
		}

		session
			.client
			.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Return done."}]}))
			.await
			.expect("turn");

		loop {
			if let ServerEvent::Notification { method, params } =
				session.events.recv().await.expect("native event")
			{
				assert_ne!(method, "error", "{params}");

				if method == "turn/completed" {
					assert_eq!(params["turn"]["status"], "completed");

					break;
				}
			}
		}

		let captured = bodies.lock().expect("captured request");

		assert_eq!(
			captured.last().expect("outbound request")["service_tier"],
			"flex",
			"configured={configured}, cold={cold}"
		);
	}

	assert_eq!(count.load(Ordering::Acquire), 2);

	for body in bodies.lock().expect("captured requests").iter() {
		assert_eq!(body["service_tier"], "flex", "configured={configured}");
	}

	backend.abort();
}
