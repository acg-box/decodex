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

#[tokio::test]
async fn checklist_observations_keep_latest_aba_and_survive_restart_without_wake() {
	let directory = tempdir().unwrap();
	let path = directory.path().join("checklist.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	store.create_chief_work_item(item("chief", None)).await.unwrap();
	store.bind_chief_thread("chief".into(), "thread".into()).await.unwrap();
	store.begin_chief_dispatch("chief".into()).await.unwrap();
	store.acknowledge_chief_dispatch("chief".into(), "turn".into()).await.unwrap();
	store.record_chief_checklist("other".into(), "turn".into(), "Wrong".into()).await.unwrap();
	store.record_chief_checklist("thread".into(), "wrong".into(), "Wrong".into()).await.unwrap();
	for text in ["A", "A", "B", "A"] {
		store.record_chief_checklist("thread".into(), "turn".into(), text.into()).await.unwrap();
	}
	let (entries, _) = store.read_chief_transcript("chief".into(), None, 32).await.unwrap();
	assert_eq!(entries.len(), 1);
	let latest_id = entries[0].id;
	assert_eq!(
		serde_json::from_str::<serde_json::Value>(&entries[0].payload).unwrap()["text"],
		"A"
	);
	assert!(
		store
			.read_chief_transcript("chief".into(), Some(latest_id), 32)
			.await
			.unwrap()
			.0
			.is_empty()
	);
	assert!(store.list_chief_wake_events("chief".into(), 32).await.unwrap().is_empty());
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	let (saved, _) = store.read_chief_transcript("chief".into(), None, 32).await.unwrap();
	assert_eq!(saved[0].id, latest_id);
	for index in 0..125 {
		store
			.record_chief_checklist("thread".into(), "turn".into(), index.to_string())
			.await
			.unwrap();
	}
	store.record_chief_checklist("thread".into(), "turn".into(), "124".into()).await.unwrap();
	let (unchanged, _) = store.read_chief_transcript("chief".into(), None, 32).await.unwrap();
	assert_eq!(
		serde_json::from_str::<serde_json::Value>(&unchanged[0].payload).unwrap()["text"],
		"124"
	);
	for index in 0..140 {
		store
			.record_chief_checklist("thread".into(), "turn".into(), index.to_string())
			.await
			.unwrap();
	}
	let (saved, _) = store.read_chief_transcript("chief".into(), None, 32).await.unwrap();
	assert_eq!(saved.len(), 1);
	assert!(saved[0].payload.contains("limit reached"));
	assert!(store.list_chief_wake_events("chief".into(), 32).await.unwrap().is_empty());
}
