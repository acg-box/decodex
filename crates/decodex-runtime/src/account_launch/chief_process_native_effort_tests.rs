//! Exact model-defined effort through native discovery and coordinator dispatch.
use super::*;
use crate::chief::{ChiefConfig, ChiefCoordinator};
use decodex_database::SqliteStore;

const EFFORT: &str = "future-provider-reasoning-effort-over-32-bytes";

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native custom-effort qualification"]
async fn installed_custom_effort_survives_catalog_and_coordinator_dispatch() {
	tokio::time::timeout(Duration::from_secs(45), qualify()).await.expect("bounded fixture");
}

async fn qualify() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir_in("/tmp").expect("fixture home");
	let catalog = home.path().join("models.json");
	let model = fixture_model("gpt-5.6-sol", EFFORT);
	std::fs::write(&catalog, serde_json::to_vec(&json!({"models":[model]})).expect("catalog JSON"))
		.expect("write catalog");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("loopback fixture");
	let address = listener.local_addr().expect("fixture address");
	let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let backend = tokio::spawn(serve_with_effort(listener, requests.clone(), Some(EFFORT)));
	std::fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_catalog_json={}\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"OpenAI\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n", serde_json::to_string(&catalog).expect("catalog path"))).expect("write config");
	let root = decodex_core::DecodexRoot::new(
		home.path().canonicalize().expect("fixture path").join("product"),
	)
	.expect("product root");
	root.paths().ensure_layout().expect("fixture layout");
	let store = SqliteStore::open(&root.paths()).expect("fixture store");
	let mut session = NativeSession::start(&binary, home.path());
	let decodex_protocol::ChiefCapabilitiesResult::Available { models, .. } =
		crate::chief_capabilities::read(&session.client).await
	else {
		panic!("native catalog available")
	};
	let model =
		models.iter().find(|model| model.model.as_str() == "gpt-5.6-sol").expect("custom model");
	assert_eq!(model.efforts[0].as_str(), EFFORT);
	assert_eq!(model.default_effort.as_ref().expect("advertised default").as_str(), EFFORT);
	let config =
		ChiefConfig::new("gpt-5.6-sol".into(), EFFORT.into(), home.path().display().to_string());
	let mut chief = ChiefCoordinator::new(store.clone(), session.client.clone(), config)
		.expect("custom effort admitted");
	let first =
		chief.start_chief("chief", "Return fixture output").await.expect("start native Chief");
	loop {
		let event = session.events.recv().await.expect("native event");
		let terminal = matches!(&event, ServerEvent::Notification { method, params } if method == "turn/completed" && params["turn"]["status"] == "completed");
		chief.handle_event(event).await.expect("native observation");
		if terminal {
			break;
		}
	}
	let thread = store
		.get_chief_work_item("chief".into())
		.await
		.expect("work")
		.codex_thread_id
		.expect("bound native thread");
	let guard = session.client.thread_settings_guard(&thread).expect("live native settings");
	let settings = session
		.client
		.thread_model_settings(&thread, guard)
		.await
		.expect("native settings read")
		.expect("supported native settings");
	assert_eq!(settings.model.as_deref(), Some("gpt-5.6-sol"));
	assert_eq!(settings.reasoning_effort.as_deref(), Some(EFFORT));
	assert_eq!(settings.model_provider.as_deref(), Some("fixture"));
	assert_eq!(requests.load(Ordering::Acquire), 1, "one original request only");
	drop(chief);
	drop(session);
	let mut session = NativeSession::start(&binary, home.path());
	let config = ChiefConfig::new(
		"unrelated-startup-model".into(),
		"low".into(),
		home.path().display().to_string(),
	);
	let mut chief = ChiefCoordinator::new(store.clone(), session.client.clone(), config)
		.expect("restarted coordinator");
	let second =
		chief.continue_worker("chief", "Continue the saved task").await.expect("cold continuation");
	loop {
		let event = session.events.recv().await.expect("native event after restart");
		let terminal = matches!(&event, ServerEvent::Notification { method, params } if method == "turn/completed" && params["turn"]["status"] == "completed");
		chief.handle_event(event).await.expect("resumed native observation");
		if terminal {
			break;
		}
	}
	assert_eq!(requests.load(Ordering::Acquire), 2, "one continuation, no replay");
	for turn in [first.active_turn_id.expect("initial turn"), second] {
		let selected = store
			.chief_turn_execution("chief".into(), thread.clone(), turn)
			.await
			.expect("execution lookup")
			.expect("atomic native ACK selection");
		assert_eq!(selected.model, "gpt-5.6-sol");
		assert_eq!(selected.effort.as_deref(), Some(EFFORT));
	}
	assert!(!backend.is_finished(), "fixture server must not fail an effort assertion");
	backend.abort();
}

pub(super) fn fixture_model(slug: &str, effort: &str) -> Value {
	json!({
		"slug":slug,"display_name":"Fixture","description":"Synthetic model",
		"default_reasoning_level":effort,"supported_reasoning_levels":[{"effort":effort,"description":"Custom"}],
		"shell_type":"shell_command","visibility":"list","minimal_client_version":"0.1.0",
		"supported_in_api":true,"priority":0,"support_verbosity":false,"default_verbosity":null,
		"apply_patch_tool_type":null,"truncation_policy":{"mode":"bytes","limit":10000},
		"supports_image_detail_original":false,"multi_agent_version":"v2","context_window":272000,
		"max_context_window":272000,"experimental_supported_tools":[],
		"model_messages":{"instructions_template":"Synthetic fixture","instructions_variables":null}
	})
}
