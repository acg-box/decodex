use super::*;

#[tokio::test]
async fn every_proven_capacity_refusal_preserves_failed_delivery_after_reopen() {
	use crate::ChiefDispatchRefusal as Refusal;
	for refusal in [
		Refusal::ServerDraining,
		Refusal::ManagedProviderChanged,
		Refusal::SettingsChanged,
		Refusal::RequestTooLarge,
		Refusal::RequestQueueFull,
	] {
		let directory = tempdir().unwrap();
		let path = directory.path().join("refusal.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store.create_chief_work_item(item("chief", None)).await.unwrap();
		store.bind_chief_thread("chief".into(), "thread".into()).await.unwrap();
		let input = store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: "input".into(),
				work_item_id: "chief".into(),
				event_kind: "user_message".into(),
				payload: r#"{"text":"Keep the original"}"#.into(),
			})
			.await
			.unwrap();
		store.begin_chief_dispatch_with_input("chief".into(), vec![input.id], None).await.unwrap();
		store.acknowledge_chief_dispatch("chief".into(), "failed".into()).await.unwrap();
		let event = store
			.complete_chief_turn_with_event(
				"chief".into(),
				"failed".into(),
				capacity_failure("chief", "failed"),
			)
			.await
			.unwrap();
		let previous = store.get_chief_work_item("chief".into()).await.unwrap();
		store.begin_chief_capacity_retry("chief".into(), event.id, i64::MAX).await.unwrap();
		store.reject_chief_dispatch(previous, None, Some(event.id), refusal).await.unwrap();
		drop(store);
		let reopened = SqliteStore::open_test(&path).unwrap();
		assert_eq!(
			reopened.get_chief_inbox_event(input.id).await.unwrap().delivered_turn_id.as_deref(),
			Some("failed")
		);
		assert!(reopened.pending_chief_capacity_retry("chief".into()).await.unwrap().is_none());
		assert!(
			reopened.begin_chief_capacity_retry("chief".into(), event.id, i64::MAX).await.is_err()
		);
		let work = reopened.get_chief_work_item("chief".into()).await.unwrap();
		assert_eq!(work.status, ChiefWorkStatus::UserDecision);
		assert_eq!(work.dispatch_state, ChiefDispatchState::Idle);
	}
}
