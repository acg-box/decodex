use super::*;

#[tokio::test]
async fn large_wake_batch_preserves_whole_events_and_leaves_remainder_unclaimed() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	complete(&mut coordinator, "chief").await;
	let mut events = Vec::new();
	for index in 0..80 {
		if index == 40 {
			coordinator
				.store
				.begin_chief_dispatch_with_events(
					"chief".into(),
					events.iter().map(|event: &ChiefInboxEvent| event.id).collect(),
				)
				.await
				.unwrap();
			coordinator
				.store
				.acknowledge_chief_dispatch("chief".into(), "previous".into())
				.await
				.unwrap();
			coordinator.store.complete_chief_turn("chief".into(), "previous".into()).await.unwrap();
		}
		events.push(
			coordinator
				.store
				.enqueue_chief_event(EnqueueChiefEvent {
					source_event_id: format!("large:{index}"),
					work_item_id: "chief".into(),
					event_kind: "automation_result".into(),
					payload: "\"".repeat(64_000),
				})
				.await
				.unwrap(),
		);
	}
	while sent.try_recv().is_ok() {}
	coordinator.wake_pending().await.unwrap();
	let chief = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let inbox = coordinator
		.store
		.list_chief_events_for_turn(chief.active_turn_id.unwrap(), 1000)
		.await
		.unwrap();
	assert!(!inbox.is_empty());
	assert_eq!(inbox[0].id, events[40].id);
	assert!(inbox.len() < 40);
	assert!(json!(inbox).to_string().len() <= MAX_WAKE_BATCH_BYTES);
	for (index, event) in events.iter().enumerate() {
		let saved = coordinator.store.get_chief_inbox_event(event.id).await.unwrap();
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
		assert!(request.to_string().len() < 2 * 1024 * 1024);
		if request["method"] == "turn/start" {
			starts.push(request);
		}
	}
	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["threadId"], json!(chief.codex_thread_id));
}

#[tokio::test]
async fn later_wake_carries_unhandled_evidence_without_replaying_worker() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	coordinator.create_worker("chief", "worker", "Inspect").await.unwrap();
	complete(&mut coordinator, "chief").await;
	complete(&mut coordinator, "worker").await;
	let previous = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let event = coordinator
		.store
		.list_chief_events_for_turn(previous.active_turn_id.unwrap(), 10)
		.await
		.unwrap()
		.remove(0);
	complete(&mut coordinator, "chief").await;
	coordinator.wake_pending().await.unwrap();
	assert!(
		coordinator
			.store
			.get_chief_work_item("chief".into())
			.await
			.unwrap()
			.active_turn_id
			.is_none()
	);
	while sent.try_recv().is_ok() {}
	coordinator
		.ingest_automation_result("new-signal", "chief", json!({"result":"Check outstanding work"}))
		.await
		.unwrap();
	let chief = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let inbox =
		coordinator.tool(&chief, &json!({"tool":"chief_list_work","arguments":{}})).await.unwrap();
	assert!(inbox["inbox"].as_array().unwrap().iter().any(|entry| entry["id"] == event.id));
	coordinator.tool(&chief, &json!({"tool":"chief_disposition","arguments":{"id":"worker","status":"resolved","eventIds":[event.id],"summary":"Accepted saved evidence"}})).await.unwrap();
	let saved = coordinator.store.get_chief_inbox_event(event.id).await.unwrap();
	assert_eq!(saved.source_event_id, event.source_event_id);
	assert_eq!(saved.payload, event.payload);
	assert_eq!(saved.disposition, Some(ChiefDisposition::Resolved));
	let mut starts = Vec::new();
	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "thread/start");
		if request["method"] == "turn/start" {
			starts.push(request);
		}
	}
	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["threadId"], json!(chief.codex_thread_id));
}

#[tokio::test]
async fn user_turn_preserves_plain_text_and_can_inspect_earlier_unhandled_results() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	coordinator.create_worker("chief", "worker", "Inspect").await.unwrap();
	complete(&mut coordinator, "chief").await;
	complete(&mut coordinator, "worker").await;
	let previous = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let event = coordinator
		.store
		.list_chief_events_for_turn(previous.active_turn_id.unwrap(), 10)
		.await
		.unwrap()
		.remove(0);
	complete(&mut coordinator, "chief").await;
	while sent.try_recv().is_ok() {}
	coordinator
		.enqueue_user_message("chief", "follow-up", "Discuss the earlier result.")
		.await
		.unwrap();
	coordinator.wake_pending().await.unwrap();
	let chief = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let inbox =
		coordinator.tool(&chief, &json!({"tool":"chief_list_work","arguments":{}})).await.unwrap();
	assert!(inbox["inbox"].as_array().unwrap().iter().any(|entry| entry["id"] == event.id));
	coordinator.tool(&chief, &json!({"tool":"chief_disposition","arguments":{"id":"worker","status":"resolved","eventIds":[event.id],"summary":"Accepted existing evidence"}})).await.unwrap();
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
		coordinator.store.get_chief_inbox_event(event.id).await.unwrap().disposition,
		Some(ChiefDisposition::Resolved)
	);
}
