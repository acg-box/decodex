//! Qualify restoration of persisted active goals whose native thread is unloaded.
use super::*;
use futures_util::FutureExt as _;
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY; isolated native goal cold recovery"]
async fn native_active_goal_on_unloaded_thread_resumes_without_local_turn_submission() {
	let native_home = tempfile::tempdir().unwrap();
	let home = native_home.path().canonicalize().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(native_permissions::serve(listener, Arc::clone(&calls)));
	std::fs::write(home.join("config.toml"),format!(
  "model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[features]\ngoals = true\n[model_providers.fixture]\nname = \"Isolated goal recovery\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n"
 )).unwrap();
	let (mut chief, _, store_home) = fixture().await;
	chief.config =
		ChiefConfig::new("gpt-5.6-sol".into(), "medium".into(), home.display().to_string());
	for phase in 0..3 {
		if phase > 0 {
			let root = decodex_core::DecodexRoot::new(
				store_home.path().canonicalize().unwrap().join("root"),
			)
			.unwrap();
			chief.store = SqliteStore::open(&root.paths()).unwrap();
		}
		let mut command =
			tokio::process::Command::new(std::env::var("DECODEX_NATIVE_BINARY").unwrap());
		command
			.arg("app-server")
			.current_dir(&home)
			.env_clear()
			.env("HOME", &home)
			.env("CODEX_HOME", &home)
			.env("PATH", "/usr/bin:/bin");
		let (client, mut events, mut process) = AppServerClient::spawn(&mut command).unwrap();
		chief = ChiefCoordinator::new(chief.store.clone(), client, chief.config.clone()).unwrap();
		let result = std::panic::AssertUnwindSafe(tokio::time::timeout(std::time::Duration::from_secs(15), async {
   chief.initialize().await.unwrap();
   if phase == 0 {
    chief.start_chief("chief", "Reply Done.").await.unwrap();
    finish(&mut chief, &mut events).await;
   }
   let work = chief.store.get_chief_work_item("chief".into()).await.unwrap();
   let thread = work.codex_thread_id.unwrap();
   assert_eq!(work.dispatch_state, decodex_database::ChiefDispatchState::Idle);
   if phase < 2 {
    chief.client.request("thread/goal/set", json!({"threadId":thread,"objective":"Persisted cold goal","status":if phase == 1 {"active"} else {"paused"},"tokenBudget":1})).await.unwrap();
    assert_eq!(calls.load(Ordering::Acquire), 1);
   } else {
    assert_eq!(chief.client.request("thread/goal/get",json!({"threadId":thread})).await.unwrap()["goal"]["status"],"active");
    assert_eq!(calls.load(Ordering::Acquire), 1);
    chief.recover_persisted().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), finish(&mut chief, &mut events)).await.expect("persisted active goal must resume");
    assert_eq!(calls.load(Ordering::Acquire), 2);
    let response = chief.client.request("thread/goal/get",json!({"threadId":thread})).await.unwrap();
    let goal: decodex_protocol::ChiefNativeGoal = serde_json::from_value(response["goal"].clone()).unwrap();
    assert!(goal.is_valid());
    assert_eq!(goal.thread_id,thread);
    assert_eq!(goal.status,"budgetLimited");
    assert_eq!(goal.tokens_used,5);
    assert_eq!(goal.token_budget,Some(1));
    assert_eq!(chief.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state, decodex_database::ChiefDispatchState::Idle);
   }
  })).catch_unwind().await;
		process.shutdown().await.unwrap();
		if !matches!(result, Ok(Ok(()))) {
			backend.abort();
		}
		result
			.expect("native goal recovery fixture panicked")
			.expect("native goal recovery fixture timed out");
	}
	backend.abort();
}

async fn finish(
	chief: &mut ChiefCoordinator,
	events: &mut tokio::sync::mpsc::Receiver<ServerEvent>,
) {
	loop {
		let event = events.recv().await.unwrap();
		let done = matches!(&event, ServerEvent::Notification { method, params } if method == "turn/completed" && params["turn"]["status"] == "completed");
		chief.handle_event(event).await.unwrap();
		if done {
			return;
		}
	}
}

#[tokio::test]
async fn goal_recovery_hydrates_only_exact_active_unloaded_thread() {
	for goal in [
		json!({"threadId":"opaque thread/1","status":"active"}),
		json!({"threadId":"opaque thread/1","status":"paused"}),
		json!({"threadId":"foreign","status":"active"}),
		Value::Null,
	] {
		let expected = goal["threadId"] == "opaque thread/1" && goal["status"] == "active";
		let (mut chief, mut sent, _home) = fixture_with_history(json!({"_goal":goal})).await;
		chief.start_chief("chief", "Initial input").await.unwrap();
		complete(&mut chief, "chief").await;
		chief.loaded_threads.clear();
		while sent.try_recv().is_ok() {}
		chief.recover_native_turns().await.unwrap();
		chief.recover_native_turns().await.unwrap();
		let mut resumes = 0;
		while let Ok(request) = sent.try_recv() {
			assert!(
				["thread/goal/get", "thread/read", "thread/resume"]
					.contains(&request["method"].as_str().unwrap())
			);
			if request["method"] == "thread/resume" {
				resumes += 1;
				assert_eq!(
					request["params"],
					json!({"threadId":"opaque thread/1","excludeTurns":true,"experimentalRawEvents":true})
				);
			}
		}
		assert_eq!(resumes, usize::from(expected));
		assert_eq!(chief.loaded_threads.contains("opaque thread/1"), expected);
		assert_eq!(
			chief.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
			decodex_database::ChiefDispatchState::Idle
		);
	}
}
