use serde_json::Value;

use crate::{
	SqliteStore,
	agent::{AgentDispatchState, AgentDisposition, EnqueueAgentEvent, tests},
};

#[tokio::test]
async fn newer_native_turn_cancels_pending_capacity_retry_without_claiming_input() {
	let directory = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("retry.sqlite3")).unwrap();

	store.create_agent_work_item(tests::item("agent", None)).await.unwrap();
	store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();
	store.begin_agent_dispatch("agent".into()).await.unwrap();
	store.acknowledge_agent_dispatch("agent".into(), "old".into()).await.unwrap();

	let mut failure = tests::capacity_failure("agent", "old");
	let mut payload: Value = serde_json::from_str(&failure.payload).unwrap();

	payload["terminal"]["threadId"] = "thread".into();
	payload["terminal"]["turn"]["id"] = "old".into();
	failure.payload = payload.to_string();

	let receipt =
		store.complete_agent_turn_with_event("agent".into(), "old".into(), failure).await.unwrap();

	assert!(
		!store
			.observe_agent_native_turn("thread".into(), "old".into(), None, "one".into())
			.await
			.unwrap()
	);
	assert!(store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_some());
	assert!(
		store
			.agent_native_terminal_recorded("agent".into(), "thread".into(), "old".into())
			.await
			.unwrap()
	);
	for (work, thread, turn) in
		[("other", "thread", "old"), ("agent", "other", "old"), ("agent", "thread", "new")]
	{
		assert!(
			!store
				.agent_native_terminal_recorded(work.into(), thread.into(), turn.into())
				.await
				.unwrap()
		);
	}

	let input = store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "input".into(),
			work_item_id: "agent".into(),
			event_kind: "user_message".into(),
			payload: "{}".into(),
		})
		.await
		.unwrap();

	assert!(
		store
			.observe_agent_native_turn("thread".into(), "new".into(), None, "one".into())
			.await
			.unwrap()
	);
	assert!(store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_none());
	assert_eq!(
		store.get_agent_inbox_event(receipt.id).await.unwrap().disposition,
		Some(AgentDisposition::Resolved)
	);

	let input = store.get_agent_inbox_event(input.id).await.unwrap();

	assert!(input.disposition.is_none());
	assert!(input.delivered_turn_id.is_none());

	store
		.complete_agent_turn_with_event(
			"agent".into(),
			"new".into(),
			tests::capacity_failure("agent", "new"),
		)
		.await
		.unwrap();

	let retry = store.pending_agent_capacity_retry("agent".into()).await.unwrap().unwrap();

	assert_eq!(retry.failed_turn_id, "new");
	assert_eq!(retry.attempt, 1);

	store.revalidate().await.unwrap();
}

#[tokio::test]
async fn native_turn_receipts_preserve_pending_input_and_reject_replay_after_restart() {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("native-turn.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();

	store.create_agent_work_item(tests::item("agent", None)).await.unwrap();
	store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();

	let pending = store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "unsent-input".into(),
			work_item_id: "agent".into(),
			event_kind: "user_message".into(),
			payload: "{}".into(),
		})
		.await
		.unwrap();

	assert!(
		!store
			.observe_agent_native_turn("foreign".into(), "turn".into(), None, "one".into())
			.await
			.unwrap()
	);
	assert!(
		store
			.observe_agent_native_turn("thread".into(), "turn".into(), None, "one".into())
			.await
			.unwrap()
	);
	assert!(
		!store
			.observe_agent_native_turn("thread".into(), "other".into(), None, "one".into())
			.await
			.unwrap()
	);

	let work = store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(work.active_turn_id.as_deref(), Some("turn"));

	let event = store.get_agent_inbox_event(pending.id).await.unwrap();

	assert!(event.disposition.is_none());
	assert!(event.delivered_turn_id.is_none());

	store.complete_agent_turn("agent".into(), "turn".into()).await.unwrap();

	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();

	assert!(
		!store
			.observe_agent_native_turn("thread".into(), "turn".into(), None, "two".into())
			.await
			.unwrap()
	);
	assert_eq!(
		store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		AgentDispatchState::Idle
	);
	assert!(
		store
			.observe_agent_native_turn("thread".into(), "next".into(), None, "two".into())
			.await
			.unwrap()
	);
	assert_eq!(store.list_pending_agent_events(20).await.unwrap().len(), 1);

	store.complete_agent_turn("agent".into(), "next".into()).await.unwrap();
	store.begin_agent_dispatch_with_events("agent".into(), vec![pending.id]).await.unwrap();

	for state in [AgentDispatchState::Dispatching, AgentDispatchState::Unknown] {
		if state == AgentDispatchState::Unknown {
			store.mark_agent_dispatch_unknown("agent".into()).await.unwrap();
		}

		assert!(
			!store
				.observe_agent_native_turn("thread".into(), "unclaimed".into(), None, "two".into())
				.await
				.unwrap()
		);
		assert_eq!(store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state, state);
		assert_eq!(
			store.get_agent_inbox_event(pending.id).await.unwrap().delivered_turn_id.as_deref(),
			Some("")
		);
	}

	store.revalidate().await.unwrap();
}
