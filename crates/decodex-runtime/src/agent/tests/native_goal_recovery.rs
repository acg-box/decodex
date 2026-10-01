//! Qualify restoration of persisted active goals whose native thread is unloaded.
use std::{
	env, fs,
	panic::AssertUnwindSafe,
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
	time::Duration,
};

use futures_util::FutureExt as _;
use tokio::{net::TcpListener, process::Command, sync::mpsc::Receiver, time};

use crate::agent::tests::*;
use decodex_core::DecodexRoot;

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY; isolated native goal cold recovery"]
async fn native_active_goal_on_unloaded_thread_resumes_without_local_turn_submission() {
	let native_home = tempfile::tempdir().unwrap();
	let home = native_home.path().canonicalize().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(native_goal_fixture::serve(listener, Arc::clone(&calls)));

	fs::write(home.join("config.toml"),format!(
  "model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[features]\ngoals = true\n[model_providers.fixture]\nname = \"Isolated goal recovery\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n"
 )).unwrap();

	let (mut agent, _, store_home) = fixture().await;

	agent.config =
		AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), home.display().to_string());

	for phase in 0..3 {
		if phase > 0 {
			let root =
				DecodexRoot::new(store_home.path().canonicalize().unwrap().join("root")).unwrap();

			agent.store = SqliteStore::open(&root.paths()).unwrap();
		}

		let mut command = Command::new(env::var("DECODEX_NATIVE_BINARY").unwrap());

		command
			.arg("app-server")
			.current_dir(&home)
			.env_clear()
			.env("HOME", &home)
			.env("CODEX_HOME", &home)
			.env("PATH", "/usr/bin:/bin");

		let (client, mut events, mut process) = AppServerClient::spawn(&mut command).unwrap();

		agent = AgentCoordinator::new(agent.store.clone(), client, agent.config.clone()).unwrap();

		let result = AssertUnwindSafe(time::timeout(Duration::from_secs(15), async {
   agent.initialize().await.unwrap();

   if phase == 0 {
    agent.start_agent("agent", "Reply Done.").await.unwrap();

    finish(&mut agent, &mut events).await;
   }

   let work = agent.store.get_agent_work_item("agent".into()).await.unwrap();
   let thread = work.codex_thread_id.unwrap();

   assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Idle);

   if phase < 2 {
    agent.client.request("thread/goal/set", json!({"threadId":thread,"objective":"Persisted cold goal","status":if phase == 1 {"active"} else {"paused"},"tokenBudget":1})).await.unwrap();

    assert_eq!(calls.load(Ordering::Acquire), 1);
   } else {
    assert_eq!(agent.client.request("thread/goal/get",json!({"threadId":thread})).await.unwrap()["goal"]["status"],"active");
    assert_eq!(calls.load(Ordering::Acquire), 1);

    agent.recover_persisted().await.unwrap();

    time::timeout(Duration::from_secs(3), finish(&mut agent, &mut events)).await.expect("persisted active goal must resume");

    assert_eq!(calls.load(Ordering::Acquire), 2);

    let goal = agent.client.thread_goal(&thread).await.unwrap().unwrap();

    assert_eq!(goal.thread_id,thread);
    assert_eq!(goal.status,decodex_codex::app_server_client::NativeThreadGoalStatus::BudgetLimited);
    assert_eq!(goal.tokens_used,5);
    assert_eq!(goal.token_budget,Some(1));
    assert_eq!(agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state, decodex_database::AgentDispatchState::Idle);
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

async fn finish(agent: &mut AgentCoordinator, events: &mut Receiver<ServerEvent>) {
	loop {
		let event = events.recv().await.unwrap();
		let done = matches!(&event, ServerEvent::Notification { method, params } if method == "turn/completed" && params["turn"]["status"] == "completed");

		agent.handle_event(event).await.unwrap();

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
		json!({"threadId":"opaque thread/1","status":"blocked"}),
		json!({"threadId":"opaque thread/1","status":"budgetLimited"}),
		json!({"threadId":"opaque thread/1","status":"usageLimited"}),
		json!({"threadId":"opaque thread/1","status":"complete"}),
		json!({"threadId":"foreign","status":"active"}),
		Value::Null,
	] {
		let mut goal = goal;

		if let Some(object) = goal.as_object_mut() {
			for (key, value) in json!({"objective":"Fixture goal","tokenBudget":null,"tokensUsed":0,"timeUsedSeconds":0,"createdAt":1,"updatedAt":1}).as_object().unwrap() {
                object.insert(key.clone(), value.clone());
            }
		}

		let expected = goal["threadId"] == "opaque thread/1" && goal["status"] == "active";
		let (mut agent, mut sent, _home) = fixture_with_history(json!({"_goal":goal})).await;

		agent.start_agent("agent", "Initial input").await.unwrap();

		complete(&mut agent, "agent").await;

		agent.loaded_threads.clear();

		while sent.try_recv().is_ok() {}

		agent.recover_native_turns().await.unwrap();
		agent.recover_native_turns().await.unwrap();

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
		assert_eq!(agent.loaded_threads.contains("opaque thread/1"), expected);
		assert_eq!(
			agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
			decodex_database::AgentDispatchState::Idle
		);
	}
}

#[tokio::test]
async fn native_goal_capacity_failure_does_not_create_a_local_retry() {
	let failure = json!({"id":"native-turn","status":"failed","error":{"message":"Capacity unavailable","codexErrorInfo":"serverOverloaded"},"items":[]});
	let (mut agent, mut sent, _home) = fixture_with_history(
		json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[failure]}}}),
	)
	.await;

	agent.start_agent("agent", "Original input").await.unwrap();

	complete(&mut agent, "agent").await;

	while sent.try_recv().is_ok() {}

	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/started".into(),
			params: json!({"threadId":"opaque thread/1","turn":{"id":"native-turn","status":"inProgress"}}),
		})
		.await
		.unwrap();
	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: json!({"threadId":"opaque thread/1","turn":failure}),
		})
		.await
		.unwrap();

	assert!(agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_none());

	agent.check_due_followups(i64::MAX).await.unwrap();

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start");
	}
}
