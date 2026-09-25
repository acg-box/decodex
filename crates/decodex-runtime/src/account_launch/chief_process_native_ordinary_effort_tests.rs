//! Ordinary typed turns preserve native reasoning and exact recovery identity.
use super::*;
use decodex_codex::{
	ConversationThreadResumeRequest, ConversationTurnInput, ConversationTurnStartRequest,
	ExactThreadId, decode_conversation_thread_resume_response,
};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated ordinary effort qualification"]
async fn installed_ordinary_turn_effort_preserves_inheritance_across_restart() {
	for (requested, configured) in [
		(None, Some("provider-effort")),
		(None, None),
		(Some("none"), Some("provider-effort")),
		(Some("provider-effort"), None),
	] {
		tokio::time::timeout(Duration::from_secs(30), qualify(requested, configured, false, false))
			.await
			.expect("bounded native ordinary effort fixture");
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native inheritance qualification"]
async fn installed_ordinary_model_inherits_but_configured_flex_is_not_effective() {
	tokio::time::timeout(
		Duration::from_secs(30),
		qualify(None, Some("provider-effort"), true, false),
	)
	.await
	.expect("bounded native inheritance fixture");
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated per-turn tier qualification"]
async fn installed_ordinary_advertised_flex_reaches_per_turn_request() {
	tokio::time::timeout(
		Duration::from_secs(30),
		qualify(None, Some("provider-effort"), true, true),
	)
	.await
	.expect("bounded per-turn tier fixture");
}

async fn qualify(
	requested: Option<&'static str>,
	configured: Option<&'static str>,
	inherit: bool,
	tier_override: bool,
) {
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
	let model_name = if inherit { "fixture-selected" } else { "gpt-5.6-sol" };
	let mut model = effort::fixture_model(model_name, "provider-effort");
	model["default_reasoning_level"] = Value::Null;
	if tier_override {
		model["service_tiers"] =
			json!([{ "id":"flex", "name":"Flex", "description":"Synthetic Flex capability" }]);
	}
	let catalog = home.path().join("models.json");
	std::fs::write(
		&catalog,
		serde_json::to_vec(&json!({"models":[model]})).expect("native ordinary effort fixture"),
	)
	.expect("native ordinary effort fixture");
	let reasoning = configured
		.map(|value| format!("model_reasoning_effort={}\n", json!(value)))
		.unwrap_or_default();
	std::fs::write(home.path().join("config.toml"), format!("{reasoning}model={}\nservice_tier=\"flex\"\nmodel_catalog_json={}\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"OpenAI\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n",json!(model_name),json!(catalog))).expect("native ordinary effort fixture");
	let mut session = NativeSession::start(&binary, home.path());
	let started = session
		.client
		.thread_start(json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only"}))
		.await
		.expect("native ordinary effort fixture");
	if inherit {
		assert_eq!(started["serviceTier"], if tier_override { json!("flex") } else { Value::Null });
	}
	let id = started["thread"]["id"].as_str().expect("native ordinary effort fixture").to_owned();
	let first_client_id = "50000000-0000-4000-8000-000000000001";
	let second_client_id = "50000000-0000-4000-8000-000000000002";
	let first_turn =
		send(&mut session, &id, requested, first_client_id, inherit, tier_override).await;
	assert_readback(&session, &id, first_client_id, &first_turn).await;
	assert_settings(&session, &id, model_name, requested.or(configured)).await;
	assert_eq!(requests.load(Ordering::Acquire), 1);
	drop(session);
	let mut session = NativeSession::start(&binary, home.path());
	assert_settings(&session, &id, model_name, requested.or(configured)).await;
	assert_eq!(requests.load(Ordering::Acquire), 1, "cold settings read cannot infer");
	let resume = ConversationThreadResumeRequest::new(
		ExactThreadId::new(&id).expect("exact saved thread"),
		"deliberately-stale-model",
		home.path().to_str().expect("fixture directory"),
		"Continue the existing task.",
	)
	.expect("typed resume request")
	.inherit_model()
	.inherit_service_tier();
	let response = session
		.client
		.thread_resume(serde_json::to_value(&resume).expect("typed resume wire"))
		.await
		.expect("native ordinary effort fixture");
	let decoded = decode_conversation_thread_resume_response(
		&resume,
		&serde_json::to_vec(&response).expect("native resume response"),
	)
	.expect("inheritance accepts actual native model while checking thread and directory");
	assert_eq!(decoded.model().as_str(), model_name);
	assert_readback(&session, &id, first_client_id, &first_turn).await;
	assert_eq!(requests.load(Ordering::Acquire), 1, "restart and readback cannot replay input");
	let second_turn =
		send(&mut session, &id, requested, second_client_id, inherit, tier_override).await;
	assert_ne!(first_turn, second_turn);
	assert_readback(&session, &id, first_client_id, &first_turn).await;
	assert_readback(&session, &id, second_client_id, &second_turn).await;
	assert_eq!(requests.load(Ordering::Acquire), 2, "one turn per call, no replay");
	assert!(!backend.is_finished(), "backend effort assertions must pass");
	for body in bodies.lock().expect("captured inference bodies").iter() {
		assert_eq!(body["reasoning"]["effort"], json!(requested.or(configured)));
		if inherit {
			assert_eq!(body["model"], model_name);
			if tier_override {
				assert_eq!(body["service_tier"], "flex");
			} else {
				assert_eq!(
					body.get("service_tier"),
					None,
					"configured Flex did not become an effective native thread tier"
				);
			}
		}
	}
	backend.abort();
}

async fn send(
	session: &mut NativeSession,
	id: &str,
	requested: Option<&str>,
	client_id: &str,
	inherit: bool,
	tier_override: bool,
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
	let request = if inherit { request.inherit_model().inherit_service_tier() } else { request };
	let mut wire = serde_json::to_value(request).expect("native ordinary effort fixture");
	if inherit && tier_override {
		wire.as_object_mut()
			.expect("native turn object")
			.insert("serviceTierForTurn".into(), json!("flex"));
	}
	let turn = session.client.turn_start(wire).await.expect("native ordinary effort fixture");
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

async fn assert_settings(
	session: &NativeSession,
	thread: &str,
	expected_model: &str,
	expected_effort: Option<&str>,
) {
	let guard = session
		.client
		.history_guard(session.client.history_revision())
		.expect("current connection history");
	let settings = session
		.client
		.thread_model_settings(thread, guard)
		.await
		.expect("exact settings read")
		.expect("installed thread settings support");
	assert_eq!(settings.model.as_deref(), Some(expected_model));
	assert_eq!(settings.model_provider.as_deref(), Some("fixture"));
	assert_eq!(settings.reasoning_effort.as_deref(), expected_effort);
}
