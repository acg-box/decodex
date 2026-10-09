use serde_json::Value;

use crate::{
	SqliteStore,
	agent::{EnqueueAgentEvent, tests},
};

fn reference_payload(thread: &str) -> String {
	serde_json::json!({"text":"Read the selected task","source":"user","options":{"taskReferences":[
		{"workId":"target","threadId":thread,"title":"Task title"}
	]}})
	.to_string()
}

async fn allowed(store: &SqliteStore, recipient: &str, thread: &str) -> bool {
	store.agent_has_task_reference(recipient.into(), "target".into(), thread.into()).await.unwrap()
}

#[tokio::test]
async fn grant_requires_delivery_and_survives_reopen_without_following_new_thread() {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("references.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();

	for id in ["recipient", "target"] {
		store.create_agent_work_item(tests::item(id, None)).await.unwrap();
		store.bind_agent_thread(id.into(), format!("{id}-thread")).await.unwrap();
	}

	let event = store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "selected-task".into(),
			work_item_id: "recipient".into(),
			event_kind: "user_message".into(),
			payload: reference_payload("target-thread"),
		})
		.await
		.unwrap();

	assert!(!allowed(&store, "recipient", "target-thread").await);
	let references = store.agent_context_references().await.unwrap();
	assert_eq!(references.len(), 1);
	assert!(references[0].delivery_turn_id.is_none());

	store.begin_agent_dispatch_with_events("recipient".into(), vec![event.id]).await.unwrap();

	assert!(!allowed(&store, "recipient", "target-thread").await);

	store.acknowledge_agent_dispatch("recipient".into(), "turn".into()).await.unwrap();

	assert!(allowed(&store, "recipient", "target-thread").await);
	let references = store.agent_context_references().await.unwrap();
	assert_eq!(references[0].delivery_turn_id.as_deref(), Some("turn"));
	assert_eq!(references[0].source_thread_id, "target-thread");
	assert!(!allowed(&store, "target", "target-thread").await);
	assert!(!allowed(&store, "recipient", "new-thread").await);

	// Seed a migration completed by an older release; current code never upgrades threads.
	store
		.run(|connection| {
			connection
				.execute_batch(
					"UPDATE agent_work_items SET codex_thread_id='new-thread' WHERE id='target';
				 INSERT INTO agent_thread_revisions(work_id,old_thread_id,new_thread_id,created_at_micros)
				 VALUES('target','target-thread','new-thread',1);",
				)
				.map_err(crate::error::sqlite_error)?;

			Ok(())
		})
		.await
		.unwrap();

	drop(store);

	let reopened = SqliteStore::open_test(&path).unwrap();

	assert!(allowed(&reopened, "recipient", "target-thread").await);
	assert!(!allowed(&reopened, "recipient", "new-thread").await);
}

#[tokio::test]
async fn rejected_unknown_and_stale_steering_never_grant_access() {
	let directory = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("references.sqlite3")).unwrap();

	for id in ["recipient", "target"] {
		store.create_agent_work_item(tests::item(id, None)).await.unwrap();
		store.bind_agent_thread(id.into(), format!("{id}-thread")).await.unwrap();
	}

	store.begin_agent_dispatch("recipient".into()).await.unwrap();
	store.acknowledge_agent_dispatch("recipient".into(), "turn".into()).await.unwrap();

	assert!(
		store
			.begin_agent_steer(
				"recipient".into(),
				"turn".into(),
				"stale".into(),
				reference_payload("stale")
			)
			.await
			.is_err()
	);

	let pending = store
		.begin_agent_steer(
			"recipient".into(),
			"turn".into(),
			"pending".into(),
			reference_payload("target-thread"),
		)
		.await
		.unwrap();

	assert!(!allowed(&store, "recipient", "target-thread").await);

	store.finish_agent_steer(pending, false).await.unwrap();

	assert!(!allowed(&store, "recipient", "target-thread").await);

	let accepted = store
		.begin_agent_steer(
			"recipient".into(),
			"turn".into(),
			"accepted".into(),
			reference_payload("target-thread"),
		)
		.await
		.unwrap();

	store.finish_agent_steer(accepted, true).await.unwrap();

	assert!(allowed(&store, "recipient", "target-thread").await);
}

#[tokio::test]
async fn invalid_reference_input_is_atomic_and_legacy_text_does_not_grant() {
	let directory = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("references.sqlite3")).unwrap();

	for id in ["recipient", "target"] {
		store.create_agent_work_item(tests::item(id, None)).await.unwrap();
		store.bind_agent_thread(id.into(), format!("{id}-thread")).await.unwrap();
	}

	let valid: Value = serde_json::from_str(&reference_payload("target-thread")).unwrap();
	let mut invalid = valid.clone();

	invalid["options"]["taskReferences"] = serde_json::json!([
		valid["options"]["taskReferences"][0],
		valid["options"]["taskReferences"][0]
	]);

	for (i, payload) in [
		invalid,
		serde_json::json!({"source":"assistant","options":valid["options"]}),
		serde_json::json!({"source":"user","options":{"taskReferences":"target"}}),
	]
	.iter()
	.enumerate()
	{
		assert!(
			store
				.enqueue_agent_event(EnqueueAgentEvent {
					source_event_id: format!("bad-{i}"),
					work_item_id: "recipient".into(),
					event_kind: "user_message".into(),
					payload: payload.to_string()
				})
				.await
				.is_err()
		);
	}

	assert!(store.read_agent_work_events("recipient".into(), 100).await.unwrap().is_empty());

	let legacy = store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "legacy".into(),
			work_item_id: "recipient".into(),
			event_kind: "user_message".into(),
			payload: "Legacy text".into(),
		})
		.await
		.unwrap();

	store.begin_agent_dispatch_with_events("recipient".into(), vec![legacy.id]).await.unwrap();
	store.acknowledge_agent_dispatch("recipient".into(), "turn".into()).await.unwrap();

	assert!(!allowed(&store, "recipient", "target-thread").await);
}
