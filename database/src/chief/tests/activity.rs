use super::*;

#[tokio::test]
async fn activity_is_idempotent_turn_bound_and_never_wakes_work() {
	let directory = tempdir().unwrap();
	let path = directory.path().join("activity.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	store.create_chief_work_item(item("chief", None)).await.unwrap();
	store.bind_chief_thread("chief".into(), "thread".into()).await.unwrap();
	store.begin_chief_dispatch("chief".into()).await.unwrap();
	store.acknowledge_chief_dispatch("chief".into(), "turn".into()).await.unwrap();
	let payload = serde_json::json!({"turn_id":"turn","item_id":"item"}).to_string();
	for thread in ["other", "thread"] {
		for _ in 0..2 {
			store
				.record_chief_activity(
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
	let (events, _) = store.read_chief_transcript("chief".into(), None, 32).await.unwrap();
	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_kind, "activity_started");
	assert!(store.list_chief_wake_events("chief".into(), 32).await.unwrap().is_empty());
	store
		.record_chief_activity(
			"thread".into(),
			"stale".into(),
			"other".into(),
			true,
			payload.clone(),
		)
		.await
		.unwrap();
	store
		.record_chief_activity("thread".into(), "turn".into(), "item".into(), true, payload)
		.await
		.unwrap();
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	let (events, _) = store.read_chief_transcript("chief".into(), None, 32).await.unwrap();
	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_kind, "activity_completed");
	let work = store.get_chief_work_item("chief".into()).await.unwrap();
	assert_eq!(work.dispatch_state, ChiefDispatchState::Running);
	assert_eq!(work.status, ChiefWorkStatus::Open);
}
