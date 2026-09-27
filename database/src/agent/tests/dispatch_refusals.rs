use super::*;

#[tokio::test]
async fn every_proven_capacity_refusal_preserves_failed_delivery_after_reopen() {
	use crate::AgentDispatchRefusal as Refusal;
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
		store.create_agent_work_item(item("agent", None)).await.unwrap();
		store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();
		let input = store
			.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id: "input".into(),
				work_item_id: "agent".into(),
				event_kind: "user_message".into(),
				payload: r#"{"text":"Keep the original"}"#.into(),
			})
			.await
			.unwrap();
		store.begin_agent_dispatch_with_input("agent".into(), vec![input.id], None).await.unwrap();
		store.acknowledge_agent_dispatch("agent".into(), "failed".into()).await.unwrap();
		let event = store
			.complete_agent_turn_with_event(
				"agent".into(),
				"failed".into(),
				capacity_failure("agent", "failed"),
			)
			.await
			.unwrap();
		let previous = store.get_agent_work_item("agent".into()).await.unwrap();
		store.begin_agent_capacity_retry("agent".into(), event.id, i64::MAX).await.unwrap();
		store.reject_agent_dispatch(previous, None, Some(event.id), refusal).await.unwrap();
		drop(store);
		let reopened = SqliteStore::open_test(&path).unwrap();
		assert_eq!(
			reopened.get_agent_inbox_event(input.id).await.unwrap().delivered_turn_id.as_deref(),
			Some("failed")
		);
		assert!(reopened.pending_agent_capacity_retry("agent".into()).await.unwrap().is_none());
		assert!(
			reopened.begin_agent_capacity_retry("agent".into(), event.id, i64::MAX).await.is_err()
		);
		let work = reopened.get_agent_work_item("agent".into()).await.unwrap();
		assert_eq!(work.status, AgentWorkStatus::UserDecision);
		assert_eq!(work.dispatch_state, AgentDispatchState::Idle);
	}
}
