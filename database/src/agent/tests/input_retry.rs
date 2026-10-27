use tempfile::TempDir;

use crate::{AgentDispatchState, EnqueueAgentEvent, SqliteStore, agent::tests};

async fn fixture() -> (TempDir, SqliteStore, i64, String) {
	let directory = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("retry.sqlite3")).unwrap();

	store.create_agent_manager(tests::item("agent", None)).await.unwrap();
	store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();
	store.create_agent_manager(tests::item("source", None)).await.unwrap();
	store.bind_agent_thread("source".into(), "source-thread".into()).await.unwrap();

	let payload = serde_json::json!({
		"text":"Original input", "source":"user",
		"options":{"attachments":[{"path":"/kept/image.png","image":true}],
		"taskReferences":[{"workId":"source","threadId":"source-thread","title":"Evidence"}],
		"execution":{"model":"selected-model"}}
	})
	.to_string();
	let event = store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "original-input".into(),
			work_item_id: "agent".into(),
			event_kind: "user_message".into(),
			payload: payload.clone(),
		})
		.await
		.unwrap();

	store.hold_agent_unsent_input("agent".into()).await.unwrap();

	(directory, store, event.id, payload)
}

#[tokio::test]
async fn explicit_input_retry_preserves_one_complete_record_across_restart() {
	let (directory, store, event, payload) = fixture().await;

	for (work, thread) in [("source", "source-thread"), ("agent", "other-thread")] {
		assert!(store.retry_agent_input(work.into(), thread.into(), event).await.is_err());
	}

	let (first, duplicate) = tokio::join!(
		store.retry_agent_input("agent".into(), "thread".into(), event),
		store.retry_agent_input("agent".into(), "thread".into(), event),
	);

	assert!(first.is_ok() && duplicate.is_ok());

	drop(store);

	let store = SqliteStore::open_test(&directory.path().join("retry.sqlite3")).unwrap();
	let saved = store.get_agent_inbox_event(event).await.unwrap();

	assert_eq!(saved.payload, payload);
	assert!(saved.disposition.is_some() && saved.delivered_turn_id.is_none());

	let pending = store.list_pending_agent_events(100).await.unwrap();

	assert_eq!(pending[0].payload, payload);
	assert_ne!(pending[0].id, event);

	let (history, _) = store.read_agent_transcript("agent".into(), None, 32).await.unwrap();

	assert!(!history.iter().any(|row| row.id == event));
	assert_eq!(store.list_pending_agent_events(100).await.unwrap().len(), 1);
}

#[tokio::test]
async fn input_retry_refuses_pending_claimed_and_running_inputs() {
	for claimed in [false, true] {
		let (_directory, store, event, payload) = fixture().await;

		store.retry_agent_input("agent".into(), "thread".into(), event).await.unwrap();

		let event = store.list_pending_agent_events(100).await.unwrap()[0].id;

		if claimed {
			store.begin_agent_dispatch_with_input("agent".into(), vec![event], None).await.unwrap();
		}

		assert!(store.retry_agent_input("agent".into(), "thread".into(), event).await.is_err());
		assert_eq!(store.get_agent_inbox_event(event).await.unwrap().payload, payload);
		assert_eq!(
			store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
			if claimed { AgentDispatchState::Dispatching } else { AgentDispatchState::Idle }
		);
	}

	let (_directory, store, event, _) = fixture().await;

	store.begin_agent_dispatch("agent".into()).await.unwrap();

	assert!(store.retry_agent_input("agent".into(), "thread".into(), event).await.is_err());
}

#[tokio::test]
async fn message_receipt_reads_only_the_exact_queue_identity_without_sending() {
	let (_directory, store, _, _) = fixture().await;

	assert!(!store.agent_message_received("agent".into(), "message-key".into()).await.unwrap());

	let event = store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: serde_json::json!(["user_message", "agent", "message-key"])
				.to_string(),
			work_item_id: "agent".into(),
			event_kind: "user_message".into(),
			payload: serde_json::json!({"text":"Exact original","source":"user"}).to_string(),
		})
		.await
		.unwrap();

	for (work, key, expected) in [
		("agent", "message-key", true),
		("source", "message-key", false),
		("agent", "another-key", false),
	] {
		assert_eq!(store.agent_message_received(work.into(), key.into()).await.unwrap(), expected);
	}

	let unchanged = store.get_agent_inbox_event(event.id).await.unwrap();

	assert!(unchanged.disposition.is_none() && unchanged.delivered_turn_id.is_none());
}
