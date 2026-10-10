use serde_json::Value;

use crate::{
	SqliteStore,
	agent::{AgentDispatchState, AgentWorkStatus, EnqueueAgentEvent, tests},
};

#[tokio::test]
async fn activity_is_idempotent_turn_bound_and_never_wakes_work() {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("activity.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();

	store.create_agent_work_item(tests::item("agent", None)).await.unwrap();
	store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();
	store.begin_agent_dispatch("agent".into()).await.unwrap();
	store.acknowledge_agent_dispatch("agent".into(), "turn".into()).await.unwrap();

	let payload = serde_json::json!({"turn_id":"turn","item_id":"item"}).to_string();

	for thread in ["other", "thread"] {
		for _ in 0..2 {
			store
				.record_agent_activity(
					thread.into(),
					"turn".into(),
					"item".into(),
					false,
					payload.clone(),
				)
				.await
				.unwrap();
		}
	}

	let (events, _) = store.read_agent_transcript("agent".into(), None, 32).await.unwrap();

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_kind, "activity_started");
	assert!(store.list_agent_wake_events("agent".into(), 32).await.unwrap().is_empty());

	store
		.record_agent_activity(
			"thread".into(),
			"stale".into(),
			"other".into(),
			true,
			payload.clone(),
		)
		.await
		.unwrap();
	store
		.record_agent_activity("thread".into(), "turn".into(), "item".into(), true, payload)
		.await
		.unwrap();

	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();
	let (events, _) = store.read_agent_transcript("agent".into(), None, 32).await.unwrap();

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_kind, "activity_completed");

	let work = store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(work.dispatch_state, AgentDispatchState::Running);
	assert_eq!(work.status, AgentWorkStatus::Open);
}

#[tokio::test]
async fn checklist_observations_keep_latest_aba_and_survive_restart_without_wake() {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("checklist.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();

	store.create_agent_work_item(tests::item("agent", None)).await.unwrap();
	store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();
	store.begin_agent_dispatch("agent".into()).await.unwrap();
	store.acknowledge_agent_dispatch("agent".into(), "turn".into()).await.unwrap();
	store.record_agent_checklist("other".into(), "turn".into(), "Wrong".into()).await.unwrap();
	store.record_agent_checklist("thread".into(), "wrong".into(), "Wrong".into()).await.unwrap();

	for text in ["A", "A", "B", "A"] {
		store.record_agent_checklist("thread".into(), "turn".into(), text.into()).await.unwrap();
	}

	let (entries, _) = store.read_agent_transcript("agent".into(), None, 32).await.unwrap();

	assert_eq!(entries.len(), 1);

	let latest_id = entries[0].id;

	assert_eq!(serde_json::from_str::<Value>(&entries[0].payload).unwrap()["text"], "A");
	assert!(
		store
			.read_agent_transcript("agent".into(), Some(latest_id), 32)
			.await
			.unwrap()
			.0
			.is_empty()
	);
	assert!(store.list_agent_wake_events("agent".into(), 32).await.unwrap().is_empty());

	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();
	let (saved, _) = store.read_agent_transcript("agent".into(), None, 32).await.unwrap();

	assert_eq!(saved[0].id, latest_id);

	for index in 0..125 {
		store
			.record_agent_checklist("thread".into(), "turn".into(), index.to_string())
			.await
			.unwrap();
	}

	store.record_agent_checklist("thread".into(), "turn".into(), "124".into()).await.unwrap();

	let (unchanged, _) = store.read_agent_transcript("agent".into(), None, 32).await.unwrap();

	assert_eq!(serde_json::from_str::<Value>(&unchanged[0].payload).unwrap()["text"], "124");

	for index in 0..140 {
		store
			.record_agent_checklist("thread".into(), "turn".into(), index.to_string())
			.await
			.unwrap();
	}

	let (saved, _) = store.read_agent_transcript("agent".into(), None, 32).await.unwrap();

	assert_eq!(saved.len(), 1);
	assert!(saved[0].payload.contains("limit reached"));
	assert!(store.list_agent_wake_events("agent".into(), 32).await.unwrap().is_empty());
}

#[tokio::test]
async fn delayed_activity_keeps_terminal_turn_without_changing_current_dispatch() {
	for kind in ["mcpToolCall", "commandExecution"] {
		check_delayed_activity(kind).await;
	}
}

async fn check_delayed_activity(kind: &str) {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("late-activity.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();

	store.create_agent_work_item(tests::item("agent", None)).await.unwrap();
	store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();
	store.begin_agent_dispatch("agent".into()).await.unwrap();
	store.acknowledge_agent_dispatch("agent".into(), "old".into()).await.unwrap();
	store.complete_agent_turn_with_event("agent".into(),"old".into(), EnqueueAgentEvent {
        source_event_id: serde_json::json!(["turn/completed","thread","old"]).to_string(),
        work_item_id:"agent".into(), event_kind:"agent_turn_completed".into(),
        payload:serde_json::json!({"terminal":{"threadId":"thread","turn":{"id":"old","status":"interrupted"}}}).to_string(),
    }).await.unwrap();
	store.begin_agent_dispatch("agent".into()).await.unwrap();
	store.acknowledge_agent_dispatch("agent".into(), "current".into()).await.unwrap();

	let wakes = store.list_agent_wake_events("agent".into(), 32).await.unwrap().len();

	for (thread, turn, payload_turn) in [
		("other", "old", "old"),
		("thread", "unknown", "unknown"),
		("thread", "old", "wrong"),
		("thread", "old", "old"),
		("thread", "old", "old"),
	] {
		for completed in [false, true] {
			store
				.record_agent_activity(
					thread.into(),
					turn.into(),
					"item".into(),
					completed,
					serde_json::json!({"kind":kind,"turn_id":payload_turn,"item_id":"item",
                    "status":if completed {"failed"} else {"running"},
                    "label":"Tool attempt","detail":"Exit code -1","duration_ms":0})
					.to_string(),
				)
				.await
				.unwrap();
		}
	}

	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();
	let (events, _) = store.read_agent_transcript("agent".into(), None, 32).await.unwrap();
	let activity =
		events.iter().filter(|event| event.event_kind == "activity_completed").collect::<Vec<_>>();

	assert_eq!(activity.len(), 1, "{kind}");
	assert_eq!(activity[0].delivered_turn_id.as_deref(), Some("old"));
	let payload: Value = serde_json::from_str(&activity[0].payload).unwrap();
	assert_eq!(payload["status"], "failed");
	assert_eq!(payload["detail"], "Exit code -1");
	assert_eq!(payload["duration_ms"], 0);

	let work = store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(work.active_turn_id.as_deref(), Some("current"));
	assert_eq!(work.dispatch_state, AgentDispatchState::Running);
	assert_eq!(store.list_agent_wake_events("agent".into(), 32).await.unwrap().len(), wakes);
}
