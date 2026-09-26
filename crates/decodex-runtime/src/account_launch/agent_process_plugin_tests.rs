//! One-shot task selection against an owned native transport and persistent journal.
use super::*;
use decodex_codex::app_server_client::ServerEvent;
use decodex_protocol::AgentPluginSelectionState as State;
use serde_json::{Value, json};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

#[tokio::test]
async fn plugin_owner_preserves_other_ids_and_never_replays_unknown_or_queued_selection() {
	for lost in [false, true] {
		let home = tempfile::tempdir().unwrap();
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let writes = Arc::new(AtomicUsize::new(0));
		let count = writes.clone();
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			writer.write_all(format!("{}\n",json!({"method":"thread/settings/updated","params":{"threadId":"task","threadSettings":{"disabledPluginIds":["keep@market"]}}})).as_bytes()).await.unwrap();
			while let Some(line) = lines.next_line().await.unwrap() {
				let request: Value = serde_json::from_str(&line).unwrap();
				let result = match request["method"].as_str().unwrap() {
					"test/barrier" => json!({}),
					"thread/read" => json!({"thread":{"id":"task","cwd":"/native"}}),
					"plugin/installed" =>
						json!({"marketplaces":[{"plugins":[{"id":"sample@market","name":"Sample","installed":true,"enabled":true,"availability":"AVAILABLE"}]}],"marketplaceLoadErrors":[]}),
					"thread/settings/update" => {
						count.fetch_add(1, Ordering::AcqRel);
						assert_eq!(
							request["params"],
							json!({"threadId":"task","disabledPluginIds":["keep@market","sample@market"]})
						);
						if lost {
							break;
						}
						json!({})
					},
					other => panic!("unexpected RPC {other}"),
				};
				writer
					.write_all(
						format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
					)
					.await
					.unwrap();
			}
		});
		client.request("test/barrier", json!({})).await.unwrap();
		let owned = OwnedReviewer::new(home.path(), &client, "task", "turn").await;
		owned.store.complete_agent_turn("root".into(), "turn".into()).await.unwrap();
		owned
			.store
			.record_agent_task_plugins(
				"task".into(),
				Some(GENERATION.into()),
				Some(json!({"disabledPluginIds":["keep@market"]}).to_string()),
				DIGEST.into(),
			)
			.await
			.unwrap();
		let state =
			crate::agent_plugins::read(&owned.store, || async { Some(owned.source(&owned.key)) })
				.await;
		let State::Available { review_token, disabled_plugin_ids, can_update, .. } = state else {
			panic!("{state:?}");
		};
		assert!(can_update);
		assert_eq!(disabled_plugin_ids[0].as_str(), "keep@market");
		let mut changed = owned.key.clone();
		changed.revision += 1;
		let change = || crate::agent_plugins::Change {
			thread: "task",
			review: review_token.as_str(),
			plugin: "sample@market",
			enabled: false,
			attempt_id: "attempt",
		};
		assert!(
			crate::agent_plugins::write(
				&owned.store,
				|| async { Some(owned.source(&changed)) },
				change()
			)
			.await
			.is_err()
		);
		assert_eq!(writes.load(Ordering::Acquire), 0);
		let result = crate::agent_plugins::write(
			&owned.store,
			|| async { Some(owned.source(&owned.key)) },
			change(),
		)
		.await;
		assert_eq!(result.is_ok(), !lost);
		assert_eq!(writes.load(Ordering::Acquire), 1);
		assert!(matches!(
			crate::agent_plugins::read(&owned.store, || async { Some(owned.source(&owned.key)) })
				.await,
			State::Pending { .. }
		));
		assert!(
			crate::agent_plugins::write(
				&owned.store,
				|| async { Some(owned.source(&owned.key)) },
				change()
			)
			.await
			.is_err()
		);
		assert_eq!(writes.load(Ordering::Acquire), 1);
		let receipt =
			owned.store.agent_plugin_receipt("root".into(), "task".into()).await.unwrap().unwrap();
		assert_eq!(receipt.state, if lost { "unknown" } else { "queued" });
		server.abort();
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native plugin controller and persistence"]
async fn installed_native_plugin_controller_confirms_through_coordinator_and_cold_resume() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let cwd = home.path().canonicalize().unwrap();
	let root = cwd.join("plugins/cache/test/sample/local/.codex-plugin");
	std::fs::create_dir_all(&root).unwrap();
	std::fs::create_dir(cwd.join(".git")).unwrap();
	std::fs::create_dir_all(cwd.join(".agents/plugins")).unwrap();
	std::fs::write(cwd.join(".agents/plugins/marketplace.json"), json!({"name":"test","plugins":[{"name":"sample","source":{"source":"local","path":"./plugins/cache/test/sample/local"}}]}).to_string()).unwrap();
	std::fs::write(
		root.join("plugin.json"),
		r#"{"name":"sample","description":"Isolated selection fixture"}"#,
	)
	.unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(super::super::serve_fixture(
		listener,
		calls.clone(),
		None,
		None,
		Some(json!({"input_tokens":0,"output_tokens":1,"total_tokens":1})),
		|n| json!({"type":"message","role":"assistant","id":format!("done-{n}"),"content":[{"type":"output_text","text":"Done"}]}),
	));
	let config = format!(
		"model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nplugins=true\nstep_model_switching=false\nenable_request_compression=false\n[plugins.\"sample@test\"]\nenabled=true\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n"
	);
	std::fs::write(cwd.join("config.toml"), &config).unwrap();
	let thread=tokio::time::timeout(std::time::Duration::from_secs(45),async {
  let mut session=super::super::NativeSession::start(&binary,&cwd);
  let start=session.client.thread_start(json!({"cwd":cwd,"model":"gpt-5.6-sol","approvalPolicy":"never","sandbox":"read-only"})).await.unwrap();
  let thread=start["thread"]["id"].as_str().unwrap().to_owned();
  session.client.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Say Done."}]})).await.unwrap();
  super::super::finish(&mut session.events).await;
  let owned=OwnedReviewer::new(home.path(),&session.client,&thread,"fixture-idle").await;
  owned.store.complete_agent_turn("root".into(),"fixture-idle".into()).await.unwrap();
  let (initial,guard)=session.client.observed_task_plugins(&thread).unwrap();assert!(guard.is_live());assert!(initial.disabled_plugin_ids.is_empty());
  owned.store.record_agent_task_plugins_publication(thread.clone(),Some(GENERATION.into()),Some(serde_json::to_string(&initial).unwrap()),DIGEST.into()).await.unwrap();
  let source=||async {let mut key=owned.key.clone();key.history_revision=session.client.history_revision();Some(owned.source(&key))};
  let state=crate::agent_plugins::read(&owned.store,source).await;
  let State::Available{review_token,catalog,can_update,..}=state else {panic!("{state:?}");};
  assert!(can_update);assert!(matches!(catalog,decodex_protocol::AgentPluginInventory::Available{ref plugins,..} if plugins.iter().any(|p|p.id=="sample@test" && p.installed)),"{catalog:?}");
  crate::agent_plugins::write(&owned.store,source,crate::agent_plugins::Change{thread:&thread,review:review_token.as_str(),plugin:"sample@test",enabled:false,attempt_id:"native-plugin-selection"}).await.unwrap();
  assert_eq!(owned.store.agent_plugin_receipt("root".into(),thread.clone()).await.unwrap().unwrap().state,"queued");
  let mut coordinator=crate::agent::AgentCoordinator::new(owned.store.clone(),session.client.clone(),crate::agent::AgentConfig::new("gpt-5.6-sol".into(),"medium".into(),cwd.display().to_string())).unwrap();
  coordinator.bind_native_generation(ProcessGenerationId::new(GENERATION).unwrap());
  loop {
   let event=session.events.recv().await.unwrap();
   let selected=matches!(&event,ServerEvent::Notification{method,params} if method=="thread/settings/updated" && params["threadId"]==thread && params["threadSettings"]["disabledPluginIds"]==json!(["sample@test"]));
   coordinator.handle_event(event).await.unwrap();
   if selected {break;}
  }
  assert_eq!(owned.store.agent_plugin_receipt("root".into(),thread.clone()).await.unwrap().unwrap().state,"target_observed");
  let State::Available{disabled_plugin_ids,last_outcome,..}=crate::agent_plugins::read(&owned.store,source).await else{panic!("native selection missing");};
  assert_eq!(disabled_plugin_ids[0].as_str(),"sample@test");assert_eq!(last_outcome,Some(decodex_protocol::AgentPluginOutcome::TargetObserved));
  assert!(crate::agent_plugins::write(&owned.store,source,crate::agent_plugins::Change{thread:&thread,review:review_token.as_str(),plugin:"sample@test",enabled:false,attempt_id:"replay"}).await.is_err());
  let reopened=SqliteStore::open(&owned.root.paths()).unwrap();
  assert_eq!(reopened.agent_plugin_receipt("root".into(),thread.clone()).await.unwrap().unwrap().state,"target_observed");
  assert_eq!(calls.load(Ordering::Acquire),1,"settings must not dispatch a turn");
  thread
 }).await.unwrap();
	let cold = super::super::NativeSession::start(&binary, &cwd);
	let resumed =
		cold.client.thread_resume(json!({"threadId":thread,"excludeTurns":true})).await.unwrap();
	assert_eq!(resumed["disabledPluginIds"], json!(["sample@test"]));
	let (saved, guard) = cold.client.observed_task_plugins(&thread).unwrap();
	assert!(guard.is_live());
	assert_eq!(saved.disabled_plugin_ids, ["sample@test"]);
	assert_eq!(std::fs::read_to_string(cwd.join("config.toml")).unwrap(), config);
	assert_eq!(calls.load(Ordering::Acquire), 1);
	drop(cold);
	backend.abort();
}
