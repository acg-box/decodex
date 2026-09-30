use super::*;
use crate::AgentTurnExecution;

#[tokio::test]
async fn execution_selection_is_atomic_exact_non_waking_and_not_inferred_after_reopen() {
	let directory = tempdir().unwrap();
	let path = directory.path().join("execution.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();

	store.create_agent_work_item(item("agent", None)).await.unwrap();
	store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();
	store.with_connection(|connection| {
		connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES('visible','agent','assistant_message','{}',1,'resolved','visible fixture',1)",[]).map_err(sqlite_error)?;

		Ok(())
	}).unwrap();
	store.begin_agent_dispatch("agent".into()).await.unwrap();

	assert!(
		store
			.acknowledge_agent_dispatch_with_execution(
				"agent".into(),
				"turn".into(),
				Some(AgentTurnExecution { model: "\n".into(), effort: None })
			)
			.await
			.is_err()
	);
	assert_eq!(
		store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		AgentDispatchState::Dispatching
	);
	assert!(
		store
			.agent_turn_execution("agent".into(), "thread".into(), "turn".into())
			.await
			.unwrap()
			.is_none()
	);

	let execution =
		AgentTurnExecution { model: "chosen-model".into(), effort: Some("future-effort".into()) };

	store
		.acknowledge_agent_dispatch_with_execution(
			"agent".into(),
			"turn".into(),
			Some(execution.clone()),
		)
		.await
		.unwrap();

	assert!(store.list_pending_agent_events(10).await.unwrap().is_empty());
	assert!(
		store
			.record_agent_task_models(
				"thread".into(),
				None,
				Some(serde_json::json!({"model":"chosen-model"}).to_string()),
				"a".repeat(64),
			)
			.await
			.unwrap()
			.is_some()
	);

	let (visible, _) = store.read_agent_transcript("agent".into(), None, 1).await.unwrap();

	assert_eq!(visible.len(), 1);
	assert_eq!(
		visible[0].event_kind, "assistant_message",
		"internal settings receipts must not consume the visible page"
	);
	assert!(store.list_agent_wake_events("agent".into(), 10).await.unwrap().is_empty());
	assert!(
		store
			.acknowledge_agent_dispatch_with_execution(
				"agent".into(),
				"turn".into(),
				Some(AgentTurnExecution { model: "overwritten".into(), effort: None })
			)
			.await
			.is_err()
	);

	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();

	assert_eq!(
		store.agent_turn_execution("agent".into(), "thread".into(), "turn".into()).await.unwrap(),
		Some(execution)
	);

	for (work, thread, turn) in
		[("other", "thread", "turn"), ("agent", "other", "turn"), ("agent", "thread", "other")]
	{
		assert!(
			store
				.agent_turn_execution(work.into(), thread.into(), turn.into())
				.await
				.unwrap()
				.is_none()
		);
	}

	store.complete_agent_turn("agent".into(), "turn".into()).await.unwrap();
	store.begin_agent_dispatch("agent".into()).await.unwrap();
	store.acknowledge_agent_dispatch("agent".into(), "unobserved-settings".into()).await.unwrap();

	assert!(
		store
			.agent_turn_execution("agent".into(), "thread".into(), "unobserved-settings".into())
			.await
			.unwrap()
			.is_none()
	);
}
