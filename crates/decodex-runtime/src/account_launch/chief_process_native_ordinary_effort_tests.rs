//! Ordinary typed turns preserve native reasoning and exact recovery identity.
use super::*;
use decodex_codex::{ConversationTurnInput, ConversationTurnStartRequest, ExactThreadId};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated ordinary effort qualification"]
async fn installed_ordinary_turn_effort_preserves_inheritance_across_restart() {
	for (requested, configured) in [
		(None, Some("provider-effort")),
		(None, None),
		(Some("none"), Some("provider-effort")),
		(Some("provider-effort"), None),
	] {
		tokio::time::timeout(Duration::from_secs(30), qualify(requested, configured))
			.await
			.expect("bounded native ordinary effort fixture");
	}
}

async fn qualify(requested: Option<&'static str>, configured: Option<&'static str>) {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit binary");
	let home = tempfile::tempdir_in("/tmp").expect("native ordinary effort fixture");
	let listener =
		tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("native ordinary effort fixture");
	let address = listener.local_addr().expect("native ordinary effort fixture");
	let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
	let backend = tokio::spawn(serve_fixture(
		listener,
		requests.clone(),
		requested.or(configured),
		Some(bodies.clone()),
		None,
		|serial| json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Native ordinary answer"}]}),
	));
	let mut model = effort::fixture_model("gpt-5.6-sol", "provider-effort");
	model["default_reasoning_level"] = Value::Null;
	let catalog = home.path().join("models.json");
	std::fs::write(
		&catalog,
		serde_json::to_vec(&json!({"models":[model]})).expect("native ordinary effort fixture"),
	)
	.expect("native ordinary effort fixture");
	let reasoning = configured
		.map(|value| format!("model_reasoning_effort={}\n", json!(value)))
		.unwrap_or_default();
	std::fs::write(home.path().join("config.toml"), format!("{reasoning}model=\"gpt-5.6-sol\"\nmodel_catalog_json={}\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"OpenAI\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n",json!(catalog))).expect("native ordinary effort fixture");
	let mut session = NativeSession::start(&binary, home.path());
	let started = session
		.client
		.thread_start(json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only"}))
		.await
		.expect("native ordinary effort fixture");
	let id = started["thread"]["id"].as_str().expect("native ordinary effort fixture").to_owned();
	let first_client_id = "50000000-0000-4000-8000-000000000001";
	let second_client_id = "50000000-0000-4000-8000-000000000002";
	let first_turn = send(&mut session, &id, requested, first_client_id).await;
	assert_readback(&session, &id, first_client_id, &first_turn).await;
	assert_eq!(requests.load(Ordering::Acquire), 1);
	drop(session);
	let mut session = NativeSession::start(&binary, home.path());
	session
		.client
		.thread_resume(json!({"threadId":id}))
		.await
		.expect("native ordinary effort fixture");
	assert_readback(&session, &id, first_client_id, &first_turn).await;
	assert_eq!(requests.load(Ordering::Acquire), 1, "restart and readback cannot replay input");
	let second_turn = send(&mut session, &id, requested, second_client_id).await;
	assert_ne!(first_turn, second_turn);
	assert_readback(&session, &id, first_client_id, &first_turn).await;
	assert_readback(&session, &id, second_client_id, &second_turn).await;
	assert_eq!(requests.load(Ordering::Acquire), 2, "one turn per call, no replay");
	assert!(!backend.is_finished(), "backend effort assertions must pass");
	for body in bodies.lock().expect("captured inference bodies").iter() {
		assert_eq!(body["reasoning"]["effort"], json!(requested.or(configured)));
	}
	backend.abort();
}

async fn send(
	session: &mut NativeSession,
	id: &str,
	requested: Option<&str>,
	client_id: &str,
) -> String {
	let request = ConversationTurnStartRequest::with_optional_effort(
		ExactThreadId::new(id).expect("native ordinary effort fixture"),
		ConversationTurnInput::text("Return fixture output")
			.expect("native ordinary effort fixture"),
		"gpt-5.6-sol",
		requested.map(str::to_owned),
	)
	.expect("native ordinary effort fixture")
	.with_client_user_message_id(client_id)
	.expect("stable native user message ID")
	.with_user_trigger();
	let turn = session
		.client
		.turn_start(serde_json::to_value(request).expect("native ordinary effort fixture"))
		.await
		.expect("native ordinary effort fixture");
	loop {
		let event = session.events.recv().await.expect("native ordinary effort fixture");
		if let ServerEvent::Notification { method, params } = event
			&& method == "turn/completed"
			&& params["threadId"] == id
			&& params["turn"]["id"] == turn["turn"]["id"]
		{
			assert_eq!(params["turn"]["status"], "completed");
			break;
		}
	}
	turn["turn"]["id"].as_str().expect("native provider turn ID").into()
}

async fn assert_readback(session: &NativeSession, thread_id: &str, client_id: &str, turn_id: &str) {
	let response = session
		.client
		.thread_read(json!({"threadId":thread_id,"includeTurns":true}))
		.await
		.expect("read native persisted history");
	let thread: crate::account_launch::protocol::ProtocolThread =
		serde_json::from_value(response["thread"].clone())
			.expect("decode native history using the ordinary recovery contract");
	let recovered =
		crate::account_launch::process::project_exact_submitted_turn(&thread, client_id)
			.expect("project exact native identity")
			.expect("native user client ID survives history persistence");
	assert_eq!(recovered.provider_turn_id().as_str(), turn_id);
	assert_eq!(recovered.status(), decodex_codex::ConversationTurnStatus::Completed);
	assert_eq!(recovered.assistant_text(), "Native ordinary answer");
	assert!(
		crate::account_launch::process::project_exact_submitted_turn(
			&thread,
			"50000000-0000-4000-8000-000000000099"
		)
		.expect("absent identity readback")
		.is_none()
	);
}
