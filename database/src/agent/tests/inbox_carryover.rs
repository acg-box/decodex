use super::*;

async fn enqueue(store: &SqliteStore, source: &str, owner: &str, kind: &str) -> AgentInboxEvent {
	store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: source.into(),
			work_item_id: owner.into(),
			event_kind: kind.into(),
			payload: "saved evidence".into(),
		})
		.await
		.unwrap()
}

#[tokio::test]
async fn unresolved_evidence_survives_failed_interrupted_turns_and_reopen() {
	let directory = tempdir().unwrap();
	let path = directory.path().join("agent.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();

	store.create_agent_work_item(item("agent", None)).await.unwrap();
	store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();

	let evidence = enqueue(&store, "original", "agent", "user_message").await;

	for status in ["failed", "interrupted"] {
		store.begin_agent_dispatch_with_events("agent".into(), vec![evidence.id]).await.unwrap();
		store.acknowledge_agent_dispatch("agent".into(), status.into()).await.unwrap();
		store
			.complete_agent_turn_with_event(
				"agent".into(),
				status.into(),
				EnqueueAgentEvent {
					source_event_id: status.into(),
					work_item_id: "agent".into(),
					event_kind: "agent_turn_completed".into(),
					payload: serde_json::json!({"terminal":{"turn":{"status":status}}}).to_string(),
				},
			)
			.await
			.unwrap();
	}

	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();
	let old = store.list_agent_wake_events("agent".into(), 10).await.unwrap();

	assert_eq!(old.len(), 1);
	assert_eq!(old[0].id, evidence.id);
	assert_eq!(old[0].payload, evidence.payload);
	assert_eq!(old[0].source_event_id, evidence.source_event_id);
	assert_eq!(old[0].delivered_turn_id.as_deref(), Some("interrupted"));

	let fresh = enqueue(&store, "fresh", "agent", "automation_result").await;

	assert_eq!(store.list_agent_wake_events("agent".into(), 1).await.unwrap()[0].id, fresh.id);

	let batch = store.list_agent_wake_events("agent".into(), 2).await.unwrap();

	assert_eq!(batch.iter().map(|event| event.id).collect::<Vec<_>>(), vec![fresh.id, evidence.id]);

	store
		.begin_agent_dispatch_with_events(
			"agent".into(),
			batch.iter().map(|event| event.id).collect(),
		)
		.await
		.unwrap();
	store.acknowledge_agent_dispatch("agent".into(), "next".into()).await.unwrap();

	assert_eq!(store.list_agent_events_for_turn("next".into(), 10).await.unwrap().len(), 2);
	assert!(
		store.begin_agent_dispatch_with_events("agent".into(), vec![evidence.id]).await.is_err()
	);

	store
		.complete_agent_turn_with_event(
			"agent".into(),
			"next".into(),
			EnqueueAgentEvent {
				source_event_id: "completed".into(),
				work_item_id: "agent".into(),
				event_kind: "agent_turn_completed".into(),
				payload: serde_json::json!({"terminal":{"turn":{"status":"completed"}}})
					.to_string(),
			},
		)
		.await
		.unwrap();

	assert_eq!(
		store.get_agent_inbox_event(evidence.id).await.unwrap().disposition,
		Some(AgentDisposition::Resolved)
	);
	assert!(
		store.begin_agent_dispatch_with_events("agent".into(), vec![evidence.id]).await.is_err()
	);
	assert!(
		store
			.list_agent_wake_events("agent".into(), 10)
			.await
			.unwrap()
			.iter()
			.all(|event| event.id != evidence.id)
	);
	assert!(store.list_agent_wake_events("agent".into(), 1001).await.is_err());
}

#[tokio::test]
async fn event_reclaim_rejects_foreign_owners_provider_requests_and_unknown_dispatch() {
	let directory = tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("agent.sqlite3")).unwrap();

	for id in ["agent", "other"] {
		store.create_agent_work_item(item(id, None)).await.unwrap();
		store.bind_agent_thread(id.into(), format!("thread-{id}")).await.unwrap();
	}

	let foreign = enqueue(&store, "foreign", "other", "automation_result").await;

	assert!(
		store.begin_agent_dispatch_with_events("agent".into(), vec![foreign.id]).await.is_err()
	);
	assert!(store.list_agent_wake_events("agent".into(), 10).await.unwrap().is_empty());

	let request = enqueue(&store, "request", "agent", "permission_pending").await;

	assert!(
		store.begin_agent_dispatch_with_events("agent".into(), vec![request.id]).await.is_err()
	);
	assert!(store.list_agent_wake_events("agent".into(), 10).await.unwrap().is_empty());

	let fresh = enqueue(&store, "fresh", "agent", "automation_result").await;

	store.begin_agent_dispatch_with_events("agent".into(), vec![fresh.id]).await.unwrap();
	store.mark_agent_dispatch_unknown("agent".into()).await.unwrap();

	assert!(store.list_agent_wake_events("agent".into(), 10).await.unwrap().is_empty());
	assert!(store.begin_agent_dispatch_with_events("agent".into(), vec![fresh.id]).await.is_err());
}
