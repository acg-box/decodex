//! Qualify native-admitted goal turns through Agent without local dispatch claims.
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
	let gate = Arc::new(tokio::sync::Notify::new());
	let continuation_gate = Arc::new(tokio::sync::Notify::new());
	let backend = tokio::spawn(native_goal_fixture::serve_with_gate(
		listener,
		Arc::clone(&calls),
		vec![(3, Arc::clone(&gate)), (5, Arc::clone(&continuation_gate))],
	));
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
	let mut agent = AgentCoordinator::new(
		fixture.store,
		client,
		AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), home_path.display().to_string()),
	)
	.unwrap();
	let result = std::panic::AssertUnwindSafe(tokio::time::timeout(
		std::time::Duration::from_secs(40),
		async {
			agent.initialize().await.unwrap();
			let initial = agent.start_agent("agent", "Reply Done.").await.unwrap();
			let thread = initial.codex_thread_id.unwrap();
			finish(&mut agent, &mut events, None).await;
			let pending = agent
				.store
				.enqueue_agent_event(EnqueueAgentEvent {
					source_event_id: "unsent-goal-input".into(),
					work_item_id: "agent".into(),
					event_kind: "user_message".into(),
					payload: json!({"text":"Pending local input","source":"user"}).to_string(),
				})
				.await
				.unwrap();
			agent
				.client
				.request(
					"thread/goal/set",
					json!({"threadId":thread,"objective":"Fixture goal","status":"active","tokenBudget":1}),
				)
				.await
				.unwrap();
			let automatic = finish(&mut agent, &mut events, Some(pending.id)).await;
			assert_ne!(Some(&automatic), initial.active_turn_id.as_ref());
			let goal =
				agent.client.request("thread/goal/get", json!({"threadId":thread})).await.unwrap();
			assert_eq!(goal["goal"]["status"], "budgetLimited");
			let input = agent.store.get_agent_inbox_event(pending.id).await.unwrap();
			assert!(input.disposition.is_none());
			assert!(input.delivered_turn_id.is_some());
			assert_ne!(input.delivered_turn_id.as_deref(), Some(automatic.as_str()));
			let user_turn = finish(&mut agent, &mut events, None).await;
			assert_eq!(input.delivered_turn_id.as_deref(), Some(user_turn.as_str()));
			let history = agent.store.read_agent_work_events("agent".into(), 100).await.unwrap();
			assert!(history.iter().any(|event| event.event_kind == "agent_turn_completed"
				&& serde_json::from_str::<Value>(&event.payload).unwrap()["terminal"]["turn"]["id"]
					== automatic));
			agent
				.handle_event(ServerEvent::Notification {
					method: "turn/started".into(),
					params: json!({"threadId":thread,"turn":{"id":automatic,"status":"inProgress"}}),
				})
				.await
				.unwrap();
			assert_eq!(
				agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
				decodex_database::AgentDispatchState::Idle
			);
			assert_eq!(calls.load(Ordering::Acquire), 3);
			recover_missed_goal_turn(&mut agent, &mut events, &thread, &gate).await;
			assert_eq!(calls.load(Ordering::Acquire), 4);
			continue_with_pending_input(&mut agent, &mut events, &thread, &continuation_gate).await;
		},
	))
	.catch_unwind()
	.await;
	process.shutdown().await.unwrap();
	backend.abort();
	result.expect("native goal fixture panicked").expect("native goal fixture timed out");
}

async fn recover_missed_goal_turn(
	agent: &mut AgentCoordinator,
	events: &mut tokio::sync::mpsc::Receiver<ServerEvent>,
	thread: &str,
	gate: &tokio::sync::Notify,
) {
	agent.client.request("thread/goal/clear", json!({"threadId":thread})).await.unwrap();
	agent.client.request("thread/goal/set", json!({"threadId":thread,"objective":"Missed native goal turn","status":"active","tokenBudget":1})).await.unwrap();
	let missed = loop {
		if let ServerEvent::Notification { method, params } = events.recv().await.unwrap()
			&& method == "turn/started"
		{
			break params["turn"]["id"].as_str().unwrap().to_owned();
		}
	};
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Idle
	);
	agent.recover_persisted().await.unwrap();
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().active_turn_id.as_deref(),
		Some(missed.as_str())
	);
	gate.notify_one();
	loop {
		if let ServerEvent::Notification { method, params } = events.recv().await.unwrap()
			&& method == "turn/completed"
			&& params["turn"]["id"] == missed
		{
			break;
		}
	}
	agent.recover_persisted().await.unwrap();
	agent.recover_persisted().await.unwrap();
	let events = agent.store.read_agent_work_events("agent".into(), 100).await.unwrap();
	assert_eq!(
		events
			.iter()
			.filter(|event| event.event_kind == "agent_turn_completed"
				&& serde_json::from_str::<Value>(&event.payload).unwrap()["terminal"]["turn"]["id"]
					== missed)
			.count(),
		1
	);
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Idle
	);
}

async fn finish(
	agent: &mut AgentCoordinator,
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
		agent.handle_event(event).await.unwrap();
		if let Some(turn) = started {
			assert_eq!(
				agent
					.store
					.get_agent_work_item("agent".into())
					.await
					.unwrap()
					.active_turn_id
					.as_deref(),
				Some(turn.as_str())
			);
			if let Some(event_id) = pending_input {
				let input = agent.store.get_agent_inbox_event(event_id).await.unwrap();
				assert!(input.delivered_turn_id.is_none());
				assert!(input.disposition.is_none());
			}
		}
		if let Some(turn) = done {
			return turn;
		}
	}
}

async fn continue_with_pending_input(
	agent: &mut AgentCoordinator,
	events: &mut tokio::sync::mpsc::Receiver<ServerEvent>,
	thread: &str,
	gate: &tokio::sync::Notify,
) {
	agent.client.request("thread/goal/clear", json!({"threadId":thread})).await.unwrap();
	let pending = agent
		.store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "ongoing-goal-input".into(),
			work_item_id: "agent".into(),
			event_kind: "user_message".into(),
			payload: json!({"text":"Input during ongoing goal","source":"user"}).to_string(),
		})
		.await
		.unwrap();
	agent.client.request("thread/goal/set", json!({"threadId":thread,"objective":"Continue while accepting input","status":"active","tokenBudget":6})).await.unwrap();
	let (completion, started, first, continuing) =
		hold_completion_until_next_start(agent, events).await;
	assert!(
		agent.store.get_agent_inbox_event(pending.id).await.unwrap().delivered_turn_id.is_none()
	);
	agent.handle_event(completion).await.unwrap();
	let delivered = agent.store.get_agent_inbox_event(pending.id).await.unwrap();
	assert!(delivered.disposition.is_none());
	let target = delivered.delivered_turn_id.unwrap();
	assert_ne!(target, first);
	assert_eq!(target, continuing);
	agent.handle_event(started).await.unwrap();
	gate.notify_one();
	let second = finish(agent, events, None).await;
	assert_eq!(second, target);
	let history = agent.client.thread_read_turn(thread, &target).await.unwrap();
	let items = history["thread"]["turns"]
		.as_array()
		.unwrap()
		.iter()
		.find(|turn| turn["id"] == target)
		.unwrap()["items"]
		.as_array()
		.unwrap();
	assert_eq!(
		items
			.iter()
			.filter(|item| item["type"] == "userMessage"
				&& item["content"].as_array().is_some_and(|parts| parts.iter().any(|part| {
					part["text"]
						.as_str()
						.is_some_and(|text| text.contains("Input during ongoing goal"))
				})))
			.count(),
		1
	);
	assert_eq!(
		agent.client.request("thread/goal/get", json!({"threadId":thread})).await.unwrap()["goal"]
			["status"],
		"budgetLimited"
	);
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Idle
	);
}

async fn hold_completion_until_next_start(
	agent: &mut AgentCoordinator,
	events: &mut tokio::sync::mpsc::Receiver<ServerEvent>,
) -> (ServerEvent, ServerEvent, String, String) {
	let mut completion = None;
	loop {
		let event = events.recv().await.unwrap();
		if let ServerEvent::Notification { method, params } = &event {
			if method == "turn/completed" {
				let turn = params["turn"]["id"].as_str().unwrap().to_owned();
				assert!(completion.replace((event, turn)).is_none());
				continue;
			}
			if method == "turn/started"
				&& let Some((completed, first)) = completion.take()
			{
				let continuing = params["turn"]["id"].as_str().unwrap().to_owned();
				return (completed, event, first, continuing);
			}
		}
		agent.handle_event(event).await.unwrap();
	}
}
