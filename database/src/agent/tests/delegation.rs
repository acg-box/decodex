use crate::{
	SqliteStore,
	agent::{AgentDisposition, AgentInboxEvent, EnqueueAgentEvent, tests},
};

async fn finish(store: &SqliteStore, turn: &str, instruction: Option<&str>) -> AgentInboxEvent {
	store
		.begin_agent_dispatch_with_input("chat".into(), vec![], instruction.map(str::to_owned))
		.await
		.unwrap();
	store.acknowledge_agent_dispatch("chat".into(), turn.into()).await.unwrap();
	store
		.complete_agent_turn_with_event(
			"chat".into(),
			turn.into(),
			EnqueueAgentEvent {
				source_event_id: turn.into(),
				work_item_id: "chat".into(),
				event_kind: "agent_turn_completed".into(),
				payload: serde_json::json!({"terminal":{"turn":{"status":"completed"}}})
					.to_string(),
			},
		)
		.await
		.unwrap()
}

#[tokio::test]
async fn ordinary_turns_stay_local_and_assignments_survive_waits_and_restart() {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("coordination.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	store.create_agent_work_item(tests::item("main", None)).await.unwrap();
	store.create_agent_manager(tests::item("chat", Some("main"))).await.unwrap();
	store.bind_agent_thread("chat".into(), "thread".into()).await.unwrap();
	assert_eq!(finish(&store, "ordinary", None).await.event_kind, "agent_turn_completed");
	assert!(store.list_agent_wake_events("main".into(), 100).await.unwrap().is_empty());
	let assigned = finish(&store, "assigned", Some("Research the requested alternatives")).await;
	assert_eq!(assigned.event_kind, "worker_turn_completed");
	store
		.dispose_agent_event(
			assigned.id,
			AgentDisposition::Wait,
			"Await the user's choice".into(),
			None,
		)
		.await
		.unwrap();
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	let answered = finish(&store, "answered", None).await;
	assert_eq!(answered.event_kind, "worker_turn_completed");
	store
		.dispose_agent_event(
			answered.id,
			AgentDisposition::Resolved,
			"Accepted the result".into(),
			None,
		)
		.await
		.unwrap();
	assert_eq!(finish(&store, "follow-up-chat", None).await.event_kind, "agent_turn_completed");
	assert!(store.list_agent_wake_events("main".into(), 100).await.unwrap().is_empty());
}

#[tokio::test]
async fn accepting_an_older_result_keeps_the_new_assignment() {
	let directory = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("review.sqlite3")).unwrap();
	store.create_agent_work_item(tests::item("main", None)).await.unwrap();
	store.create_agent_manager(tests::item("chat", Some("main"))).await.unwrap();
	store.bind_agent_thread("chat".into(), "thread".into()).await.unwrap();
	let first = finish(&store, "first", Some("First assignment")).await;
	let second = finish(&store, "second", Some("New assignment")).await;
	store
		.dispose_agent_event(first.id, AgentDisposition::Resolved, "First accepted".into(), None)
		.await
		.unwrap();
	assert_eq!(
		store.get_agent_work_item("chat".into()).await.unwrap().status,
		crate::AgentWorkStatus::Open
	);
	assert_eq!(finish(&store, "new-result", None).await.event_kind, "worker_turn_completed");
	store
		.dispose_agent_event(
			second.id,
			AgentDisposition::Resolved,
			"New assignment accepted".into(),
			None,
		)
		.await
		.unwrap();
	assert_eq!(finish(&store, "ordinary", None).await.event_kind, "agent_turn_completed");
}
