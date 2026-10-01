//! Qualify runtime model publication against the installed native owner.
use std::{
	env, fs,
	sync::{
		Arc, Mutex,
		atomic::{AtomicUsize, Ordering},
	},
	time::Duration,
};

use crate::account_launch::agent_process::native_tests::{
	NativeSession,
	reviewer::store::{OwnedReviewer, SqliteStore},
	serve_fixture,
};
use serde_json::{self, Value};
use tokio::{net::TcpListener, time};

use crate::agent_models::{self, Change};
use decodex_codex::app_server_client::{NativeTaskModelSettings, ServerEvent};
use decodex_protocol::{AgentModelSelectionState, ConversationReasoningEffort};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated task model selection"]
async fn installed_native_task_model_selection_preserves_other_tasks_and_current_turn() {
	qualify(false).await;
	qualify(true).await;
}

async fn qualify(plan: bool) {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().expect("native task model fixture");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("native task model fixture");
	let address = listener.local_addr().expect("native task model fixture");
	let calls = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(serve_fixture(
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

	fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_reasoning_effort=\"low\"\nmodel_provider=\"fixture\"\n[features]\nstep_model_switching=true\nenable_request_compression=false\n[model_providers.fixture]\nname=\"OpenAI\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("native task model fixture");

	let config = fs::read(home.path().join("config.toml")).expect("native task model fixture");
	let mut session = NativeSession::start(&binary, home.path());
	let thread=time::timeout(Duration::from_secs(45),async {
        let start=session.client.thread_start(serde_json::json!({"cwd":home.path(),"model":"gpt-5.6-sol","approvalPolicy":"never","sandbox":"read-only","dynamicTools":[{"name":"pause_fixture","description":"Pause this local fixture","inputSchema":{"type":"object","properties":{}}}]})).await.expect("native task model fixture");
        let thread=start["thread"]["id"].as_str().expect("native task model fixture").to_owned();
        let mut params=serde_json::json!({"threadId":thread,"effort":"low","input":[{"type":"text","text":"Run the local pause fixture."}]});

        if plan {params["collaborationMode"]=serde_json::json!({"mode":"plan","settings":{"model":"gpt-5.6-sol","reasoning_effort":"low","developer_instructions":null}});}

        let turn=session.client.turn_start(params).await.expect("native task model fixture");
        let turn=turn["turn"]["id"].as_str().expect("native task model fixture");
        let (id,method,params)=super::super::next_request(&mut session.events).await;

        assert_eq!(method,"item/tool/call");

        let guard=session.client.server_request_guard(&id,&method,&params).expect("native task model fixture");
        let owned=OwnedReviewer::new(home.path(),&session.client,&thread,turn).await;
        let initial=NativeTaskModelSettings::from_thread_response(&start).expect("complete start settings");

        agent_models::persist_current(&owned.store,&session.client,&thread,Some(owned.key.generation.as_str().into())).await.expect("native task model fixture");

        let state=agent_models::read(&owned.store,|| async {Some(owned.source(&owned.key))}).await;
        let AgentModelSelectionState::Available {review_token,models:choices,..}=state else {panic!("native task choices unavailable: {state:?}");};

        assert!(choices.iter().any(|m|m.model.as_str()=="gpt-5.6-terra" && m.efforts.contains(&ConversationReasoningEffort::High)));

        agent_models::write(&owned.store,|| async {Some(owned.source(&owned.key))},Change {thread:&thread,review:review_token.as_str(),model:"gpt-5.6-terra",effort:Some("high"),attempt_id:"native-task-model"}).await.expect("native task model fixture");

        assert_eq!(owned.store.agent_model_receipt("root".into(),thread.clone()).await.expect("queued receipt").expect("reserved selection").state,"queued");
        assert_eq!(calls.load(Ordering::Acquire),1,"selection must not release the paused tool");

        loop {
            if let ServerEvent::Notification {method,params}=session.events.recv().await.expect("native task model fixture")
                && method=="thread/settings/updated" && params["threadId"]==thread {
                let settings=NativeTaskModelSettings::from_notification(&params["threadSettings"]).expect("native task model fixture");

                if settings.model!="gpt-5.6-terra" {continue;}

                assert_eq!(settings.effort.as_deref(),Some("high"));

                if plan {assert_eq!(params["threadSettings"]["collaborationMode"]["mode"],"plan");}

                assert_eq!(settings.service_tier,initial.service_tier);

                agent_models::persist_current(&owned.store,&session.client,&thread,Some(owned.key.generation.as_str().into())).await.expect("native task model fixture");

                break;
            }
        }

        let reopened=SqliteStore::open(&owned.root.paths()).expect("native task model fixture");
        let status=reopened.agent_model_receipt("root".into(),thread.clone()).await.expect("native task model fixture").expect("native task model fixture");

        assert_eq!(status.state,"target_observed");
        assert_eq!(status.attempt.generation.as_deref(),Some(owned.key.generation.as_str()));

        session.client.respond_guarded(id,serde_json::json!({"contentItems":[{"type":"inputText","text":"Continue"}],"success":true}),guard).await.expect("native task model fixture");

        super::super::finish(&mut session.events).await;

        assert_eq!(calls.load(Ordering::Acquire),2);

        thread
    }).await.expect("native live model deadline");

	drop(session);

	let mut cold = NativeSession::start(&binary, home.path());

	time::timeout(Duration::from_secs(30),async {
        let resumed=cold.client.thread_resume(serde_json::json!({"threadId":thread})).await.expect("native task model fixture");

        if plan {assert_eq!(resumed["collaborationMode"]["mode"],"plan");}

        cold.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Confirm the following turn uses its saved settings."}]})).await.expect("native task model fixture");

        super::super::finish(&mut cold.events).await;

        let other=cold.client.thread_start(serde_json::json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only"})).await.expect("native task model fixture");

        cold.client.turn_start(serde_json::json!({"threadId":other["thread"]["id"],"input":[{"type":"text","text":"Use independent defaults."}]})).await.expect("native task model fixture");

        super::super::finish(&mut cold.events).await;
    }).await.expect("cold native model deadline");

	let captured = bodies.lock().expect("native task model fixture");

	assert_eq!(captured.len(), 4);

	for (request, (model, effort)) in captured.iter().zip([
		("gpt-5.6-sol", "low"),
		("gpt-5.6-sol", "low"),
		("gpt-5.6-terra", "high"),
		("gpt-5.6-sol", "low"),
	]) {
		assert_eq!(request["model"], model);

		let metadata: Value = serde_json::from_str(
			request["client_metadata"]["x-codex-turn-metadata"]
				.as_str()
				.expect("native task model fixture"),
		)
		.expect("native task model fixture");

		assert_eq!(metadata["model"], model);
		assert_eq!(metadata["reasoning_effort"], effort);
	}

	if plan {
		for index in [0, 1, 2] {
			assert!(
				captured[index].to_string().contains("# Plan Mode (Conversational)"),
				"Plan mode lost on request {index}"
			);
		}

		assert!(
			!captured[3].to_string().contains("# Plan Mode (Conversational)"),
			"Plan mode leaked into a new task"
		);
	}

	assert_eq!(
		fs::read(home.path().join("config.toml")).expect("native task model fixture"),
		config
	);

	drop(captured);
	drop(cold);

	backend.abort();
}
