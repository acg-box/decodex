use super::*;

#[tokio::test]
async fn native_turn_receipts_preserve_pending_input_and_reject_replay_after_restart() {
	let directory = tempdir().unwrap();
	let path = directory.path().join("native-turn.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	store.create_chief_work_item(item("chief", None)).await.unwrap();
	store.bind_chief_thread("chief".into(), "thread".into()).await.unwrap();
	let pending = store
		.enqueue_chief_event(EnqueueChiefEvent {
			source_event_id: "unsent-input".into(),
			work_item_id: "chief".into(),
			event_kind: "user_message".into(),
			payload: "{}".into(),
		})
		.await
		.unwrap();
	assert!(
		!store
			.observe_chief_native_turn("foreign".into(), "turn".into(), None, "one".into())
			.await
			.unwrap()
	);
	assert!(
		store
			.observe_chief_native_turn("thread".into(), "turn".into(), None, "one".into())
			.await
			.unwrap()
	);
	assert!(
		!store
			.observe_chief_native_turn("thread".into(), "other".into(), None, "one".into())
			.await
			.unwrap()
	);
	let work = store.get_chief_work_item("chief".into()).await.unwrap();
	assert_eq!(work.active_turn_id.as_deref(), Some("turn"));
	let event = store.get_chief_inbox_event(pending.id).await.unwrap();
	assert!(event.disposition.is_none());
	assert!(event.delivered_turn_id.is_none());
	store.complete_chief_turn("chief".into(), "turn".into()).await.unwrap();
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	assert!(
		!store
			.observe_chief_native_turn("thread".into(), "turn".into(), None, "two".into())
			.await
			.unwrap()
	);
	assert_eq!(
		store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		ChiefDispatchState::Idle
	);
	assert!(
		store
			.observe_chief_native_turn("thread".into(), "next".into(), None, "two".into())
			.await
			.unwrap()
	);
	assert_eq!(store.list_pending_chief_events(20).await.unwrap().len(), 1);
	store.complete_chief_turn("chief".into(), "next".into()).await.unwrap();
	store.begin_chief_dispatch_with_events("chief".into(), vec![pending.id]).await.unwrap();
	for state in [ChiefDispatchState::Dispatching, ChiefDispatchState::Unknown] {
		if state == ChiefDispatchState::Unknown {
			store.mark_chief_dispatch_unknown("chief".into()).await.unwrap();
		}
		assert!(
			!store
				.observe_chief_native_turn("thread".into(), "unclaimed".into(), None, "two".into())
				.await
				.unwrap()
		);
		assert_eq!(store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state, state);
		assert_eq!(
			store.get_chief_inbox_event(pending.id).await.unwrap().delivered_turn_id.as_deref(),
			Some("")
		);
	}
	store.revalidate().await.unwrap();
}
