//! Qualify native-admitted goal turns through Chief without local dispatch claims.
use super::*;
use futures_util::FutureExt as _;
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY; isolated native goal lifecycle"]
async fn native_goal_turn_preserves_separate_user_input_delivery() {
	let home = tempfile::tempdir().unwrap();
	let home_path = home.path().canonicalize().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(native_permissions::serve(listener, Arc::clone(&calls)));
	std::fs::write(home_path.join("config.toml"),format!(
		"model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[features]\ngoals = true\n[model_providers.fixture]\nname = \"Isolated goal fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n"
	)).unwrap();
	let (fixture, _, _store_home) = fixture().await;
	let mut command = tokio::process::Command::new(std::env::var("DECODEX_NATIVE_BINARY").unwrap());
	command
		.arg("app-server")
		.current_dir(&home_path)
		.env_clear()
		.env("HOME", &home_path)
		.env("CODEX_HOME", &home_path)
		.env("PATH", "/usr/bin:/bin");
	let (client, mut events, mut process) = AppServerClient::spawn(&mut command).unwrap();
	let mut chief = ChiefCoordinator::new(
		fixture.store,
		client,
		ChiefConfig::new("gpt-5.6-sol".into(), "medium".into(), home_path.display().to_string()),
	)
	.unwrap();
	let result = std::panic::AssertUnwindSafe(tokio::time::timeout(
		std::time::Duration::from_secs(40),
		async {
			chief.initialize().await.unwrap();
			let initial = chief.start_chief("chief", "Reply Done.").await.unwrap();
			let thread = initial.codex_thread_id.unwrap();
			finish(&mut chief, &mut events, None).await;
			let pending = chief
				.store
				.enqueue_chief_event(EnqueueChiefEvent {
					source_event_id: "unsent-goal-input".into(),
					work_item_id: "chief".into(),
					event_kind: "user_message".into(),
					payload: json!({"text":"Pending local input","source":"user"}).to_string(),
				})
				.await
				.unwrap();
			chief
				.client
				.request(
					"thread/goal/set",
					json!({"threadId":thread,"objective":"Fixture goal","status":"active","tokenBudget":1}),
				)
				.await
				.unwrap();
			let automatic = finish(&mut chief, &mut events, Some(pending.id)).await;
			assert_ne!(Some(&automatic), initial.active_turn_id.as_ref());
			let goal =
				chief.client.request("thread/goal/get", json!({"threadId":thread})).await.unwrap();
			assert_eq!(goal["goal"]["status"], "budgetLimited");
			let input = chief.store.get_chief_inbox_event(pending.id).await.unwrap();
			assert!(input.disposition.is_none());
			assert!(input.delivered_turn_id.is_some());
			assert_ne!(input.delivered_turn_id.as_deref(), Some(automatic.as_str()));
			let user_turn = finish(&mut chief, &mut events, None).await;
			assert_eq!(input.delivered_turn_id.as_deref(), Some(user_turn.as_str()));
			let history = chief.store.read_chief_work_events("chief".into(), 100).await.unwrap();
			assert!(history.iter().any(|event| event.event_kind == "chief_turn_completed"
				&& serde_json::from_str::<Value>(&event.payload).unwrap()["terminal"]["turn"]["id"]
					== automatic));
			chief
				.handle_event(ServerEvent::Notification {
					method: "turn/started".into(),
					params: json!({"threadId":thread,"turn":{"id":automatic,"status":"inProgress"}}),
				})
				.await
				.unwrap();
			assert_eq!(
				chief.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
				decodex_database::ChiefDispatchState::Idle
			);
			assert_eq!(calls.load(Ordering::Acquire), 3);
		},
	))
	.catch_unwind()
	.await;
	process.shutdown().await.unwrap();
	backend.abort();
	result.expect("native goal fixture panicked").expect("native goal fixture timed out");
}

async fn finish(
	chief: &mut ChiefCoordinator,
	events: &mut tokio::sync::mpsc::Receiver<ServerEvent>,
	pending_input: Option<i64>,
) -> String {
	loop {
		let event = events.recv().await.expect("native goal events");
		let started = match &event {
			ServerEvent::Notification { method, params } if method == "turn/started" =>
				params["turn"]["id"].as_str().map(str::to_owned),
			_ => None,
		};
		let done = match &event {
			ServerEvent::Notification { method, params } if method == "turn/completed" => {
				assert_eq!(params["turn"]["status"], "completed");
				params["turn"]["id"].as_str().map(str::to_owned)
			},
			_ => None,
		};
		chief.handle_event(event).await.unwrap();
		if let Some(turn) = started {
			assert_eq!(
				chief
					.store
					.get_chief_work_item("chief".into())
					.await
					.unwrap()
					.active_turn_id
					.as_deref(),
				Some(turn.as_str())
			);
			if let Some(event_id) = pending_input {
				let input = chief.store.get_chief_inbox_event(event_id).await.unwrap();
				assert!(input.delivered_turn_id.is_none());
				assert!(input.disposition.is_none());
			}
		}
		if let Some(turn) = done {
			return turn;
		}
	}
}
