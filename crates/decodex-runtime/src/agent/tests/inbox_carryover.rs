use crate::agent::tests::{
	self, AgentDisposition, AgentInboxEvent, EnqueueAgentEvent, MAX_WAKE_BATCH_BYTES,
};

#[tokio::test]
async fn large_wake_batch_preserves_whole_events_and_leaves_remainder_unclaimed() {
	let (mut coordinator, mut sent, _directory) = tests::fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();

	tests::complete(&mut coordinator, "agent").await;

	let mut events = Vec::new();

	for index in 0..80 {
		if index == 40 {
			coordinator
				.store
				.begin_agent_dispatch_with_events(
					"agent".into(),
					events.iter().map(|event: &AgentInboxEvent| event.id).collect(),
				)
				.await
				.unwrap();
			coordinator
				.store
				.acknowledge_agent_dispatch("agent".into(), "previous".into())
				.await
				.unwrap();
			coordinator.store.complete_agent_turn("agent".into(), "previous".into()).await.unwrap();
		}

		events.push(
			coordinator
				.store
				.enqueue_agent_event(EnqueueAgentEvent {
					source_event_id: format!("large:{index}"),
					work_item_id: "agent".into(),
					event_kind: "automation_result".into(),
					payload: "\"".repeat(64_000),
				})
				.await
				.unwrap(),
		);
	}

	while sent.try_recv().is_ok() {}

	coordinator.wake_pending().await.unwrap();

	let agent = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let inbox = coordinator
		.store
		.list_agent_events_for_turn(agent.active_turn_id.unwrap(), 1_000)
		.await
		.unwrap();

	assert!(!inbox.is_empty());
	assert_eq!(inbox[0].id, events[40].id);
	assert!(inbox.len() < 40);
	assert!(serde_json::json!(inbox).to_string().len() <= MAX_WAKE_BATCH_BYTES);

	for (index, event) in events.iter().enumerate() {
		let saved = coordinator.store.get_agent_inbox_event(event.id).await.unwrap();

		assert_eq!(saved.payload, event.payload);

		if index < 40 {
			assert_eq!(saved.delivered_turn_id.as_deref(), Some("previous"));
		} else if !inbox.iter().any(|entry| entry.id == event.id) {
			assert!(saved.delivered_turn_id.is_none());
		}
	}

	let mut starts = Vec::new();

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "thread/start");
		assert!(request.to_string().len() < 2 * 1_024 * 1_024);

		if request["method"] == "turn/start" {
			starts.push(request);
		}
	}

	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["threadId"], serde_json::json!(agent.codex_thread_id));
}

#[tokio::test]
async fn later_wake_carries_unhandled_evidence_without_replaying_worker() {
	let (mut coordinator, mut sent, _directory) = tests::fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();
	coordinator.create_worker("agent", "worker", "Inspect").await.unwrap();

	tests::complete(&mut coordinator, "agent").await;
	tests::complete(&mut coordinator, "worker").await;

	let previous = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let event = coordinator
		.store
		.list_agent_events_for_turn(previous.active_turn_id.unwrap(), 10)
		.await
		.unwrap()
		.remove(0);

	tests::complete(&mut coordinator, "agent").await;

	coordinator.wake_pending().await.unwrap();

	assert!(
		coordinator
			.store
			.get_agent_work_item("agent".into())
			.await
			.unwrap()
			.active_turn_id
			.is_none()
	);

	while sent.try_recv().is_ok() {}

	coordinator
		.ingest_automation_result(
			"new-signal",
			"agent",
			serde_json::json!({"result":"Check outstanding work"}),
		)
		.await
		.unwrap();

	let agent = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let inbox = coordinator
		.tool(&agent, &serde_json::json!({"tool":"agent_list_work","arguments":{}}))
		.await
		.unwrap();

	assert!(inbox["inbox"].as_array().unwrap().iter().any(|entry| entry["id"] == event.id));

	coordinator.tool(&agent, &serde_json::json!({"tool":"agent_disposition","arguments":{"id":"worker","status":"resolved","eventIds":[event.id],"summary":"Accepted saved evidence"}})).await.unwrap();

	let saved = coordinator.store.get_agent_inbox_event(event.id).await.unwrap();

	assert_eq!(saved.source_event_id, event.source_event_id);
	assert_eq!(saved.payload, event.payload);
	assert_eq!(saved.disposition, Some(AgentDisposition::Resolved));

	let mut starts = Vec::new();

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "thread/start");

		if request["method"] == "turn/start" {
			starts.push(request);
		}
	}

	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["threadId"], serde_json::json!(agent.codex_thread_id));
}

#[tokio::test]
async fn user_turn_preserves_plain_text_and_can_inspect_earlier_unhandled_results() {
	let (mut coordinator, mut sent, _directory) = tests::fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();
	coordinator.create_worker("agent", "worker", "Inspect").await.unwrap();

	tests::complete(&mut coordinator, "agent").await;
	tests::complete(&mut coordinator, "worker").await;

	let previous = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let event = coordinator
		.store
		.list_agent_events_for_turn(previous.active_turn_id.unwrap(), 10)
		.await
		.unwrap()
		.remove(0);

	tests::complete(&mut coordinator, "agent").await;

	while sent.try_recv().is_ok() {}

	coordinator
		.enqueue_user_message("agent", "follow-up", "Discuss the earlier result.")
		.await
		.unwrap();
	coordinator.wake_pending().await.unwrap();

	let agent = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let inbox = coordinator
		.tool(&agent, &serde_json::json!({"tool":"agent_list_work","arguments":{}}))
		.await
		.unwrap();

	assert!(inbox["inbox"].as_array().unwrap().iter().any(|entry| entry["id"] == event.id));

	coordinator.tool(&agent, &serde_json::json!({"tool":"agent_disposition","arguments":{"id":"worker","status":"resolved","eventIds":[event.id],"summary":"Accepted existing evidence"}})).await.unwrap();

	let mut starts = Vec::new();

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "thread/start");

		if request["method"] == "turn/start" {
			starts.push(request);
		}
	}

	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["input"][0]["text"], "Discuss the earlier result.");
	assert_eq!(
		coordinator.store.get_agent_inbox_event(event.id).await.unwrap().disposition,
		Some(AgentDisposition::Resolved)
	);
}

#[tokio::test]
async fn exhausted_account_pause_preserves_input_without_dispatch() {
	let (mut coordinator, mut sent, _directory) = tests::fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();

	tests::complete(&mut coordinator, "agent").await;

	let event = coordinator
		.store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "paused-input".into(),
			work_item_id: "agent".into(),
			event_kind: "user_message".into(),
			payload: serde_json::json!({"text":"Continue"}).to_string(),
		})
		.await
		.unwrap();

	while sent.try_recv().is_ok() {}

	coordinator.pause_dispatch(true);
	coordinator.wake_pending().await.unwrap();

	assert!(sent.try_recv().is_err());
	assert!(
		coordinator
			.store
			.get_agent_inbox_event(event.id)
			.await
			.unwrap()
			.delivered_turn_id
			.is_none()
	);

	coordinator.pause_dispatch(false);
	coordinator.wake_pending().await.unwrap();

	assert!(
		coordinator
			.store
			.get_agent_inbox_event(event.id)
			.await
			.unwrap()
			.delivered_turn_id
			.is_some()
	);
}
