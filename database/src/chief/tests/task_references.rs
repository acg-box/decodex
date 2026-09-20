use super::*;
use serde_json::json;

fn reference_payload(thread: &str) -> String {
	json!({"text":"Read the selected task","source":"user","options":{"taskReferences":[
		{"workId":"target","threadId":thread,"title":"Task title"}
	]}})
	.to_string()
}

async fn allowed(store: &SqliteStore, recipient: &str, thread: &str) -> bool {
	store.chief_has_task_reference(recipient.into(), "target".into(), thread.into()).await.unwrap()
}

#[tokio::test]
async fn grant_requires_delivery_and_survives_reopen_without_following_new_thread() {
	let directory = tempdir().unwrap();
	let path = directory.path().join("references.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	for id in ["recipient", "target"] {
		store.create_chief_work_item(item(id, None)).await.unwrap();
		store.bind_chief_thread(id.into(), format!("{id}-thread")).await.unwrap();
	}
	let event = store
		.enqueue_chief_event(EnqueueChiefEvent {
			source_event_id: "selected-task".into(),
			work_item_id: "recipient".into(),
			event_kind: "user_message".into(),
			payload: reference_payload("target-thread"),
		})
		.await
		.unwrap();
	assert!(!allowed(&store, "recipient", "target-thread").await);
	store.begin_chief_dispatch_with_events("recipient".into(), vec![event.id]).await.unwrap();
	assert!(!allowed(&store, "recipient", "target-thread").await);
	store.acknowledge_chief_dispatch("recipient".into(), "turn".into()).await.unwrap();
	assert!(allowed(&store, "recipient", "target-thread").await);
	assert!(!allowed(&store, "target", "target-thread").await);
	assert!(!allowed(&store, "recipient", "new-thread").await);
	store.begin_chief_tool_upgrade("target".into(), "target-thread".into()).await.unwrap();
	store
		.finish_chief_tool_upgrade("target".into(), "target-thread".into(), "new-thread".into())
		.await
		.unwrap();
	drop(store);
	let reopened = SqliteStore::open_test(&path).unwrap();
	assert!(allowed(&reopened, "recipient", "target-thread").await);
	assert!(!allowed(&reopened, "recipient", "new-thread").await);
}

#[tokio::test]
async fn rejected_unknown_and_stale_steering_never_grant_access() {
	let directory = tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("references.sqlite3")).unwrap();
	for id in ["recipient", "target"] {
		store.create_chief_work_item(item(id, None)).await.unwrap();
		store.bind_chief_thread(id.into(), format!("{id}-thread")).await.unwrap();
	}
	store.begin_chief_dispatch("recipient".into()).await.unwrap();
	store.acknowledge_chief_dispatch("recipient".into(), "turn".into()).await.unwrap();
	assert!(
		store
			.begin_chief_steer(
				"recipient".into(),
				"turn".into(),
				"stale".into(),
				reference_payload("stale")
			)
			.await
			.is_err()
	);
	let pending = store
		.begin_chief_steer(
			"recipient".into(),
			"turn".into(),
			"pending".into(),
			reference_payload("target-thread"),
		)
		.await
		.unwrap();
	assert!(!allowed(&store, "recipient", "target-thread").await);
	store.finish_chief_steer(pending, false).await.unwrap();
	assert!(!allowed(&store, "recipient", "target-thread").await);
	let accepted = store
		.begin_chief_steer(
			"recipient".into(),
			"turn".into(),
			"accepted".into(),
			reference_payload("target-thread"),
		)
		.await
		.unwrap();
	store.finish_chief_steer(accepted, true).await.unwrap();
	assert!(allowed(&store, "recipient", "target-thread").await);
}

#[tokio::test]
async fn invalid_reference_input_is_atomic_and_legacy_text_does_not_grant() {
	let directory = tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("references.sqlite3")).unwrap();
	for id in ["recipient", "target"] {
		store.create_chief_work_item(item(id, None)).await.unwrap();
		store.bind_chief_thread(id.into(), format!("{id}-thread")).await.unwrap();
	}
	let valid: serde_json::Value =
		serde_json::from_str(&reference_payload("target-thread")).unwrap();
	let mut invalid = valid.clone();
	invalid["options"]["taskReferences"] =
		json!([valid["options"]["taskReferences"][0], valid["options"]["taskReferences"][0]]);
	for (i, payload) in [
		invalid,
		json!({"source":"assistant","options":valid["options"]}),
		json!({"source":"user","options":{"taskReferences":"target"}}),
	]
	.iter()
	.enumerate()
	{
		assert!(
			store
				.enqueue_chief_event(EnqueueChiefEvent {
					source_event_id: format!("bad-{i}"),
					work_item_id: "recipient".into(),
					event_kind: "user_message".into(),
					payload: payload.to_string()
				})
				.await
				.is_err()
		);
	}
	assert!(store.read_chief_work_events("recipient".into(), 100).await.unwrap().is_empty());
	let legacy = store
		.enqueue_chief_event(EnqueueChiefEvent {
			source_event_id: "legacy".into(),
			work_item_id: "recipient".into(),
			event_kind: "user_message".into(),
			payload: "Legacy text".into(),
		})
		.await
		.unwrap();
	store.begin_chief_dispatch_with_events("recipient".into(), vec![legacy.id]).await.unwrap();
	store.acknowledge_chief_dispatch("recipient".into(), "turn".into()).await.unwrap();
	assert!(!allowed(&store, "recipient", "target-thread").await);
}
