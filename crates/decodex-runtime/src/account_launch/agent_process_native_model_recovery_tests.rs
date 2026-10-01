//! Qualify partial model updates without resetting mode, instructions, or permissions.
use std::{
	env, fs,
	sync::{Mutex, atomic::AtomicUsize},
};

use tokio::{net::TcpListener, time};

use crate::account_launch::agent_process::native_tests::{
	self, Arc, Duration, NativeSession, Ordering, ServerEvent, Value,
};
use decodex_codex::app_server_client::{NativeTaskModelSettings, ThreadModelRecoveryUpdate};

fn assert_resumed_settings(resumed: &Value, mode: &str) {
	assert_eq!(resumed["model"], "gpt-5.6-terra");
	assert_eq!(resumed["reasoningEffort"], "medium");
	assert_eq!(
		resumed["collaborationMode"],
		serde_json::json!({
			"mode": mode,
			"settings": {
				"model": "gpt-5.6-terra",
				"reasoning_effort": "medium",
				"developer_instructions": "Keep this exact mode instruction."
			}
		}),
		"cold resume must preserve native mode and instructions"
	);
	assert!(
		resumed["serviceTier"].is_null() || resumed["serviceTier"] == "default",
		"standard tier must not become priority after cold resume"
	);
	assert_eq!(resumed["approvalPolicy"], "on-request");
	assert_eq!(resumed["approvalsReviewer"], "user");
	assert_eq!(resumed["sandbox"]["type"], "readOnly");
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native model recovery with loopback inference"]
async fn installed_model_recovery_preserves_task_policy_and_does_not_replay_turns() {
	for mode in ["default", "plan"] {
		for preserve_tier in [false, true] {
			qualify(mode, preserve_tier).await;
		}
	}
}

async fn qualify(mode: &str, preserve_tier: bool) {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");

	assert!(std::path::Path::new(&binary).is_absolute());

	let home = tempfile::tempdir().expect("isolated model recovery home");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback inference");
	let address = listener.local_addr().expect("loopback address");
	let count = Arc::new(AtomicUsize::new(0));
	let requests = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(native_tests::serve_fixture_usage(
		listener,
		Arc::clone(&count),
		None,
		Some(Arc::clone(&requests)),
		|_| serde_json::json!({"input_tokens":1,"output_tokens":1,"total_tokens":2}),
		|serial| serde_json::json!({"type":"message","role":"assistant","id":format!("message-{serial}"),"content":[{"type":"output_text","text":"Fixture done."}]}),
	));

	fs::write(home.path().join("config.toml"),format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\n[model_providers.fixture]\nname = \"Local recovery fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).expect("isolated model configuration");

	let saved = fs::read(home.path().join("config.toml")).expect("original defaults");
	let mut session = NativeSession::start(&binary, home.path());
	let guard = session.client.thread_settings_guard("auth-fixture").expect("native source guard");

	assert_eq!(
		session.client.native_recovery_auth(guard).await.expect("native auth metadata"),
		decodex_codex::app_server_client::NativeRecoveryAuth::Inapplicable,
		"custom provider must not consume ChatGPT fallback banners"
	);

	let thread = time::timeout(Duration::from_secs(30), async {
		let started = session.client.thread_start(serde_json::json!({"cwd":home.path(),"model":"gpt-5.6-sol","approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":"read-only"})).await.expect("native task");
		let thread = started["thread"]["id"].as_str().expect("native thread");
		let initial = NativeTaskModelSettings::from_thread_response(&started).expect("complete native start settings");

		assert_eq!(initial.model,"gpt-5.6-sol");

		session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Return fixture done."}],
			"collaborationMode":{"mode":mode,"settings":{"model":"gpt-5.6-sol","reasoning_effort":"medium","developer_instructions":"Keep this exact mode instruction."}}})).await.expect("one fixture turn");

		loop {
			if let Some(ServerEvent::Notification { method, .. }) = session.events.recv().await {
				if method == "turn/completed" { break; }
			} else { panic!("native turn event stream ended"); }
		}

		assert_eq!(count.load(Ordering::Acquire),1);

		let update = if preserve_tier {
 ThreadModelRecoveryUpdate::preserving_service_tier(thread,"gpt-5.6-terra","medium")
 } else { ThreadModelRecoveryUpdate::new(thread,"gpt-5.6-terra","medium","default") }.expect("bounded recovery update");
		let guard = session.client.history_guard(session.client.history_revision()).expect("current native source");

		session.client.queue_thread_model_recovery(&update,guard).await.expect("native queue acknowledgment");

		let settings = loop {
			let Some(ServerEvent::Notification { method, params }) = session.events.recv().await else { panic!("native settings event stream ended"); };

			if method == "thread/settings/updated" && params["threadId"] == thread && params["threadSettings"]["model"] == "gpt-5.6-terra" { break params["threadSettings"].clone(); }
		};

		assert_eq!(settings["collaborationMode"]["mode"],mode);
		assert_eq!(settings["collaborationMode"]["settings"]["developer_instructions"],"Keep this exact mode instruction.");
		assert_eq!(settings["collaborationMode"]["settings"]["model"],"gpt-5.6-terra");
		assert_eq!(settings["approvalPolicy"],started["approvalPolicy"]);
		assert_eq!(settings["approvalsReviewer"],started["approvalsReviewer"]);
		assert_eq!(settings["sandboxPolicy"],started["sandbox"]);

		let expected_tier = if preserve_tier { initial.service_tier.as_deref() } else { Some("default") };

		assert_eq!(settings["serviceTier"], serde_json::json!(expected_tier));

		let published = NativeTaskModelSettings::from_notification(&settings).expect("complete native publication");

		assert_eq!(published.model,"gpt-5.6-terra");
		assert_eq!(published.service_tier.as_deref(),expected_tier);

		let guard = session.client.history_guard(session.client.history_revision()).expect("read source");
		let read = session.client.thread_model_settings(thread,guard).await.expect("native settings read").expect("installed settings fields");

		assert_eq!(read.model.as_deref(),Some("gpt-5.6-terra"));
		assert_eq!(count.load(Ordering::Acquire),1,"settings recovery must not start another inference");

		thread.to_owned()
	}).await.expect("bounded native model recovery");

	assert_eq!(fs::read(home.path().join("config.toml")).expect("current defaults"), saved);

	drop(session);

	let mut reopened = NativeSession::start(&binary, home.path());
	let resumed = time::timeout(
		Duration::from_secs(20),
		reopened.client.thread_resume(
			serde_json::json!({"threadId":thread,"excludeTurns":true,"experimentalRawEvents":true}),
		),
	)
	.await
	.expect("cold resume deadline")
	.expect("native cold resume");

	assert_eq!(resumed["thread"]["id"], thread);

	assert_resumed_settings(&resumed, mode);

	assert_eq!(count.load(Ordering::Acquire), 1, "cold hydration must not replay input");
	assert_eq!(fs::read(home.path().join("config.toml")).expect("defaults after resume"), saved);

	// A new user request with no model overrides must use the recovered task
	// settings rather than the process-level defaults from config.toml.
	time::timeout(Duration::from_secs(20), async {
		reopened
			.client
			.turn_start(
				serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Return another fixture response."}]}),
			)
			.await
			.expect("explicit continuation");

		loop {
			match reopened.events.recv().await {
				Some(ServerEvent::Notification { method, .. }) if method == "turn/completed" =>
					break,
				Some(_) => {},
				None => panic!("continuation event stream ended"),
			}
		}
	})
	.await
	.expect("continuation deadline");

	assert_eq!(count.load(Ordering::Acquire), 2);

	let bodies = requests.lock().expect("captured inference requests");

	assert_eq!(bodies.len(), 2);
	assert_eq!(bodies[1]["model"], "gpt-5.6-terra");
	assert_eq!(bodies[1]["reasoning"]["effort"], "medium");
	assert!(
		serde_json::to_string(&bodies[1])
			.expect("continuation request")
			.contains("Keep this exact mode instruction."),
		"the next explicit input must retain the restored mode instructions"
	);

	drop(bodies);
	drop(reopened);

	backend.abort();
}
