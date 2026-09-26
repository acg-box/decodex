use super::*;
use crate::ChiefTurnExecution;

#[tokio::test]
async fn execution_selection_is_atomic_exact_non_waking_and_not_inferred_after_reopen() {
	let directory = tempdir().unwrap();
	let path = directory.path().join("execution.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	store.create_chief_work_item(item("chief", None)).await.unwrap();
	store.bind_chief_thread("chief".into(), "thread".into()).await.unwrap();
	store.with_connection(|connection| {
		connection.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES('visible','chief','assistant_message','{}',1,'resolved','visible fixture',1)",[]).map_err(sqlite_error)?;
		Ok(())
	}).unwrap();
	store.begin_chief_dispatch("chief".into()).await.unwrap();
	assert!(
		store
			.acknowledge_chief_dispatch_with_execution(
				"chief".into(),
				"turn".into(),
				Some(ChiefTurnExecution { model: "\n".into(), effort: None })
			)
			.await
			.is_err()
	);
	assert_eq!(
		store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		ChiefDispatchState::Dispatching
	);
	assert!(
		store
			.chief_turn_execution("chief".into(), "thread".into(), "turn".into())
			.await
			.unwrap()
			.is_none()
	);
	let execution =
		ChiefTurnExecution { model: "chosen-model".into(), effort: Some("future-effort".into()) };
	store
		.acknowledge_chief_dispatch_with_execution(
			"chief".into(),
			"turn".into(),
			Some(execution.clone()),
		)
		.await
		.unwrap();
	assert!(store.list_pending_chief_events(10).await.unwrap().is_empty());

	assert!(
		store
			.record_chief_task_models(
				"thread".into(),
				None,
				Some(serde_json::json!({"model":"chosen-model"}).to_string()),
				"a".repeat(64),
			)
			.await
			.unwrap()
			.is_some()
	);
	let (visible, _) = store.read_chief_transcript("chief".into(), None, 1).await.unwrap();
	assert_eq!(visible.len(), 1);
	assert_eq!(
		visible[0].event_kind, "assistant_message",
		"internal settings receipts must not consume the visible page"
	);

	assert!(store.list_chief_wake_events("chief".into(), 10).await.unwrap().is_empty());
	assert!(
		store
			.acknowledge_chief_dispatch_with_execution(
				"chief".into(),
				"turn".into(),
				Some(ChiefTurnExecution { model: "overwritten".into(), effort: None })
			)
			.await
			.is_err()
	);
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	assert_eq!(
		store.chief_turn_execution("chief".into(), "thread".into(), "turn".into()).await.unwrap(),
		Some(execution)
	);
	for (work, thread, turn) in
		[("other", "thread", "turn"), ("chief", "other", "turn"), ("chief", "thread", "other")]
	{
		assert!(
			store
				.chief_turn_execution(work.into(), thread.into(), turn.into())
				.await
				.unwrap()
				.is_none()
		);
	}
	store.complete_chief_turn("chief".into(), "turn".into()).await.unwrap();
	store.begin_chief_dispatch("chief".into()).await.unwrap();
	store.acknowledge_chief_dispatch("chief".into(), "unobserved-settings".into()).await.unwrap();
	assert!(
		store
			.chief_turn_execution("chief".into(), "thread".into(), "unobserved-settings".into())
			.await
			.unwrap()
			.is_none()
	);
}
