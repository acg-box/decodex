//! Qualify runtime model publication against the installed native owner.
#[path = "agent_process_native_child_model_tests.rs"] mod child_model;

use std::{
	env, fs,
	sync::{
		Arc, Mutex,
		atomic::{AtomicUsize, Ordering},
	},
	time::Duration,
};

use serde_json::Value;
use tokio::{net::TcpListener, time};

use crate::{
	account_launch::agent_process::native_tests::{
		self, NativeSession,
		reviewer::store::{OwnedReviewer, SqliteStore},
	},
	agent_live_settings::{self, LiveEdit},
};
use decodex_codex::app_server_client::ServerEvent;
use decodex_protocol::{AgentLiveReviewerState, ConversationModel, ConversationReasoningEffort};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated live model switching"]
async fn installed_native_live_model_publication_preserves_future_defaults() {
	qualify_live_model(true).await;
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated disabled live model feature"]
async fn installed_native_disabled_live_model_preserves_all_steps() {
	qualify_live_model(false).await;
}

async fn qualify_live_model(enabled: bool) {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().expect("native live-model fixture");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("native live-model fixture");
	let address = listener.local_addr().expect("native live-model fixture");
	let calls = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		calls.clone(),
		None,
		Some(bodies.clone()),
		Some(serde_json::json!({"input_tokens":1,"output_tokens":1,"total_tokens":2})),
		|serial| {
			if serial == 0 {
				serde_json::json!({"type":"function_call","name":"pause_fixture","arguments":"{}","call_id":"pause"})
			} else {
				serde_json::json!({"type":"message","role":"assistant","id":format!("done-{serial}"),"content":[{"type":"output_text","text":"Done"}]})
			}
		},
	));

	fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_reasoning_effort=\"low\"\nmodel_provider=\"fixture\"\n[features]\nstep_model_switching={enabled}\nenable_request_compression=false\n[model_providers.fixture]\nname=\"OpenAI\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("native live-model fixture");

	let config = fs::read(home.path().join("config.toml")).expect("native live-model fixture");
	let mut session = NativeSession::start(&binary, home.path());
	let thread=time::timeout(Duration::from_secs(45),async {
        let start=session.client.thread_start(serde_json::json!({"cwd":home.path(),"model":"gpt-5.6-sol","approvalPolicy":"never","sandbox":"read-only","dynamicTools":[{"name":"pause_fixture","description":"Pause this local fixture","inputSchema":{"type":"object","properties":{}}}]})).await.expect("native live-model fixture");
        let thread=start["thread"]["id"].as_str().expect("native live-model fixture").to_owned();
        let turn=session.client.turn_start(serde_json::json!({"threadId":thread,"effort":"low","input":[{"type":"text","text":"Run the local pause fixture."}]})).await.expect("native live-model fixture");
        let turn=turn["turn"]["id"].as_str().expect("native live-model fixture");
        let (id,method,params)=super::super::next_request(&mut session.events).await;

        assert_eq!(method,"item/tool/call");

        let guard=session.client.server_request_guard(&id,&method,&params).expect("native live-model fixture");
        let owned=OwnedReviewer::new(home.path(),&session.client,&thread,turn).await;
        let state=agent_live_settings::read_options(&owned.store,true,|| async {Some(owned.source(&owned.key))}).await;
        let AgentLiveReviewerState::Available {review_token,model_choices,..}=state else {panic!("native live choices unavailable: {state:?}");};

        assert_eq!(model_choices.is_some(),enabled);

        if let Some(choices)=model_choices {assert!(choices.iter().any(|m|m.model.as_str()=="gpt-5.6-terra" && m.efforts.contains(&ConversationReasoningEffort::High)));}

        let unsupported=agent_live_settings::write(&owned.store,|| async {Some(owned.source(&owned.key))},turn,review_token.as_str(),LiveEdit::Model {model:ConversationModel::new("absent-from-native-catalog").expect("native live-model fixture"),effort:ConversationReasoningEffort::High},"unsupported-model-edit").await;

        assert!(matches!(unsupported,Err(crate::agent_host::AgentHostError::Rejected(_))));
        assert!(owned.store.agent_live_settings_receipt("root".into(),thread.clone(),turn.into()).await.expect("native live-model fixture").is_none());

        let published=agent_live_settings::write(&owned.store,|| async {Some(owned.source(&owned.key))},turn,review_token.as_str(),LiveEdit::Model {model:ConversationModel::new("gpt-5.6-terra").expect("native live-model fixture"),effort:ConversationReasoningEffort::High},"native-model-edit").await;

        if enabled {published.expect("native live-model fixture");} else {assert!(matches!(published,Err(crate::agent_host::AgentHostError::Rejected(_))));}

        assert_eq!(calls.load(Ordering::Acquire),1,"publication must not release the paused tool");

        let reopened=SqliteStore::open(&owned.root.paths()).expect("native live-model fixture");
        let receipt=reopened.agent_live_settings_receipt("root".into(),thread.clone(),turn.into()).await.expect("native live-model fixture");

        if enabled {assert_eq!(receipt.expect("native live-model fixture").outcome,"applied");} else {assert!(receipt.is_none());}

        // Another native client reads persisted task defaults without resuming or sending input.
        let observer=NativeSession::start(&binary,home.path());
        let settings=observer.client.thread_model_settings(&thread,observer.client.history_guard(0).expect("native live-model fixture")).await.expect("native live-model fixture").expect("native live-model fixture");

        assert_eq!(settings.model.as_deref(),Some("gpt-5.6-sol"));
        assert_eq!(settings.reasoning_effort.as_deref(),Some("low"));
        assert_eq!(calls.load(Ordering::Acquire),1,"read-only observer must not run the model");

        drop(observer);

        session.client.respond_guarded(id,serde_json::json!({"contentItems":[{"type":"inputText","text":"Continue"}],"success":true}),guard).await.expect("native live-model fixture");

        super::super::finish(&mut session.events).await;

        assert_eq!(calls.load(Ordering::Acquire),2);

        thread
    }).await.expect("native live model deadline");

	drop(session);

	let mut cold = NativeSession::start(&binary, home.path());

	time::timeout(Duration::from_secs(30),async {
        cold.client.thread_resume(serde_json::json!({"threadId":thread})).await.expect("native live-model fixture");
        cold.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Confirm the following turn uses its saved settings."}]})).await.expect("native live-model fixture");

        super::super::finish(&mut cold.events).await;
    }).await.expect("cold native model deadline");

	let captured = bodies.lock().expect("native live-model fixture");

	assert_eq!(captured.len(), 3);

	for (request, (model, effort)) in captured.iter().zip([
		("gpt-5.6-sol", "low"),
		if enabled { ("gpt-5.6-terra", "high") } else { ("gpt-5.6-sol", "low") },
		("gpt-5.6-sol", "low"),
	]) {
		assert_eq!(request["model"], model);

		let metadata: Value = serde_json::from_str(
			request["client_metadata"]["x-codex-turn-metadata"]
				.as_str()
				.expect("native live-model fixture"),
		)
		.expect("native live-model fixture");

		assert_eq!(metadata["model"], model);
		assert_eq!(metadata["reasoning_effort"], effort);
	}

	assert_eq!(
		fs::read(home.path().join("config.toml")).expect("native live-model fixture"),
		config
	);

	drop(captured);
	drop(cold);

	backend.abort();
}
