use super::*;

#[tokio::test]
async fn native_steer_receipts_require_exact_identity_and_survive_reopen() {
	let directory = tempdir().unwrap();
	let path = directory.path().join("steer.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	store.create_chief_work_item(item("work", None)).await.unwrap();
	store.bind_chief_thread("work".into(), "thread".into()).await.unwrap();
	store.begin_chief_dispatch("work".into()).await.unwrap();
	store.acknowledge_chief_dispatch("work".into(), "turn".into()).await.unwrap();
	let payload = serde_json::json!({"text":"Identical input","source":"user"}).to_string();
	let first = store
		.begin_chief_steer("work".into(), "turn".into(), "first".into(), payload.clone())
		.await
		.unwrap();
	let second = store
		.begin_chief_steer("work".into(), "turn".into(), "second".into(), payload)
		.await
		.unwrap();
	for (thread, turn, client, generation) in [
		("foreign", "turn", "second", None),
		("thread", "wrong", "second", None),
		("thread", "turn", "other", None),
		("thread", "turn", "second", Some("unbound".into())),
	] {
		store
			.observe_chief_steer_receipt(thread.into(), turn.into(), client.into(), generation)
			.await
			.unwrap();
		assert!(store.get_chief_inbox_event(second).await.unwrap().disposition.is_none());
	}
	store.complete_chief_turn("work".into(), "turn".into()).await.unwrap();
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	for _ in 0..2 {
		store
			.observe_chief_steer_receipt("thread".into(), "turn".into(), "second".into(), None)
			.await
			.unwrap();
	}
	assert!(store.get_chief_inbox_event(first).await.unwrap().disposition.is_none());
	assert!(store.get_chief_inbox_event(second).await.unwrap().disposition.is_some());
	// A delayed RPC reply cannot turn an already confirmed receipt into an error.
	store.finish_chief_steer(second, true).await.unwrap();
	for (work, thread, turn, key, expected) in [
		("work", "thread", "turn", "second", true),
		("work", "thread", "turn", "first", false),
		("foreign", "thread", "turn", "second", false),
		("work", "foreign", "turn", "second", false),
		("work", "thread", "foreign", "second", false),
	] {
		assert_eq!(
			store
				.chief_steer_confirmed(work.into(), thread.into(), turn.into(), key.into())
				.await
				.unwrap(),
			expected
		);
	}
	let events = store.list_chief_events_for_turn("turn".into(), 100).await.unwrap();
	assert_eq!(events.iter().filter(|e| e.event_kind == "user_message").count(), 1);
	assert!(store.list_undelivered_chief_events(100).await.unwrap().is_empty());
	store.finish_chief_steer(first, false).await.unwrap();
	store
		.observe_chief_steer_receipt("thread".into(), "turn".into(), "first".into(), None)
		.await
		.unwrap();
	let events = store.list_chief_events_for_turn("turn".into(), 100).await.unwrap();
	assert_eq!(events.iter().filter(|e| e.event_kind == "user_message").count(), 1);
}
