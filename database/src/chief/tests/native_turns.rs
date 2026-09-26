use super::*;

#[tokio::test]
async fn newer_native_turn_cancels_pending_capacity_retry_without_claiming_input() {
	let directory = tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("retry.sqlite3")).unwrap();
	store.create_chief_work_item(item("chief", None)).await.unwrap();
	store.bind_chief_thread("chief".into(), "thread".into()).await.unwrap();
	store.begin_chief_dispatch("chief".into()).await.unwrap();
	store.acknowledge_chief_dispatch("chief".into(), "old".into()).await.unwrap();
	let mut failure = capacity_failure("chief", "old");
	let mut payload: serde_json::Value = serde_json::from_str(&failure.payload).unwrap();
	payload["terminal"]["threadId"] = "thread".into();
	payload["terminal"]["turn"]["id"] = "old".into();
	failure.payload = payload.to_string();
	let receipt =
		store.complete_chief_turn_with_event("chief".into(), "old".into(), failure).await.unwrap();
	assert!(
		!store
			.observe_chief_native_turn("thread".into(), "old".into(), None, "one".into())
			.await
			.unwrap()
	);
	assert!(store.pending_chief_capacity_retry("chief".into()).await.unwrap().is_some());
	let input = store
		.enqueue_chief_event(EnqueueChiefEvent {
			source_event_id: "input".into(),
			work_item_id: "chief".into(),
			event_kind: "user_message".into(),
			payload: "{}".into(),
		})
		.await
		.unwrap();
	assert!(
		store
			.observe_chief_native_turn("thread".into(), "new".into(), None, "one".into())
			.await
			.unwrap()
	);
	assert!(store.pending_chief_capacity_retry("chief".into()).await.unwrap().is_none());
	assert_eq!(
		store.get_chief_inbox_event(receipt.id).await.unwrap().disposition,
		Some(ChiefDisposition::Resolved)
	);
	let input = store.get_chief_inbox_event(input.id).await.unwrap();
	assert!(input.disposition.is_none());
	assert!(input.delivered_turn_id.is_none());
	store
		.complete_chief_turn_with_event(
			"chief".into(),
			"new".into(),
			capacity_failure("chief", "new"),
		)
		.await
		.unwrap();
	let retry = store.pending_chief_capacity_retry("chief".into()).await.unwrap().unwrap();
	assert_eq!(retry.failed_turn_id, "new");
	assert_eq!(retry.attempt, 1);
	store.revalidate().await.unwrap();
}

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
