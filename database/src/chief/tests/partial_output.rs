use super::*;

#[tokio::test]
async fn terminal_partial_output_survives_restart_and_next_turn_without_waking() {
	let directory = tempdir().unwrap();
	let path = directory.path().join("partial.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	store.create_chief_work_item(item("chief", None)).await.unwrap();
	store.bind_chief_thread("chief".into(), "thread".into()).await.unwrap();
	store.begin_chief_dispatch("chief".into()).await.unwrap();
	store.acknowledge_chief_dispatch("chief".into(), "turn".into()).await.unwrap();
	let source = "Intro.\n\n$$\n\\frac{a+b}{c}";
	for (id, kind, completed) in [
		("answer", "agentMessage", false),
		("plan", "plan", false),
		("final", "agentMessage", true),
	] {
		store
			.update_chief_output_record(crate::ChiefOutputUpdate {
				thread_id: "thread".into(),
				turn_id: "turn".into(),
				item_id: id.into(),
				kind: kind.into(),
				text: source.into(),
				completed,
			})
			.await
			.unwrap();
	}
	store.complete_chief_turn_with_event("chief".into(),"turn".into(),EnqueueChiefEvent {
        source_event_id:"terminal".into(),work_item_id:"chief".into(),event_kind:"chief_turn_completed".into(),payload:r#"{"terminal":{"threadId":"thread","turn":{"id":"turn","status":"interrupted"}}}"#.into()
    }).await.unwrap();
	assert!(store.read_chief_output("chief".into()).await.unwrap().is_empty());
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	store.begin_chief_dispatch("chief".into()).await.unwrap();
	store.acknowledge_chief_dispatch("chief".into(), "next".into()).await.unwrap();
	store
		.update_chief_output(
			"thread".into(),
			"next".into(),
			"new".into(),
			"New answer".into(),
			false,
		)
		.await
		.unwrap();
	let (events, live) = store.read_chief_transcript("chief".into(), None, 10).await.unwrap();
	let retained: Vec<_> = events.iter().filter(|e| e.event_kind == "partial_output").collect();
	assert_eq!(retained.len(), 2);
	for event in retained {
		let value: serde_json::Value = serde_json::from_str(&event.payload).unwrap();
		assert_eq!(value["text"], source);
		assert_ne!(value["itemId"], "final");
		assert_eq!(event.disposition, Some(ChiefDisposition::Resolved));
		let (page, _) =
			store.read_chief_transcript("chief".into(), Some(event.id + 1), 1).await.unwrap();
		assert_eq!(page[0].id, event.id);
	}
	assert_eq!(live[0].text, "New answer");
	assert!(store.list_pending_chief_events(10).await.unwrap().is_empty());
	store
		.update_chief_output(
			"thread".into(),
			"turn".into(),
			"answer".into(),
			"Authoritative final".into(),
			true,
		)
		.await
		.unwrap();
	let (events, _) = store.read_chief_transcript("chief".into(), None, 10).await.unwrap();
	assert_eq!(
		events.iter().filter(|e| e.event_kind == "partial_output").count(),
		2,
		"late completion cannot erase fallback text before native history is available"
	);
	store
		.invalidate_chief_output("thread".into(), Some("foreign-generation".into()))
		.await
		.unwrap();
	let (events, _) = store.read_chief_transcript("chief".into(), None, 10).await.unwrap();
	assert_eq!(events.iter().filter(|event| event.event_kind == "partial_output").count(), 2);
	store.invalidate_chief_output("thread".into(), None).await.unwrap();
	let (events, live) = store.read_chief_transcript("chief".into(), None, 10).await.unwrap();
	assert!(!events.iter().any(|event| event.event_kind == "partial_output"));
	assert!(live.is_empty());
}

#[tokio::test]
async fn partial_output_bounds_escaped_text_for_each_terminal_status() {
	for status in ["completed", "failed", "interrupted"] {
		let directory = tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("bounded.sqlite3")).unwrap();
		store.create_chief_work_item(item("chief", None)).await.unwrap();
		store.bind_chief_thread("chief".into(), "thread".into()).await.unwrap();
		store.begin_chief_dispatch("chief".into()).await.unwrap();
		store.acknowledge_chief_dispatch("chief".into(), "turn".into()).await.unwrap();
		let text = "🦀\u{0001}".repeat(13000);
		store
			.update_chief_output(
				"thread".into(),
				"turn".into(),
				"answer".into(),
				text.clone(),
				false,
			)
			.await
			.unwrap();
		store
			.complete_chief_turn_with_event(
				"chief".into(),
				"turn".into(),
				EnqueueChiefEvent {
					source_event_id: "terminal".into(),
					work_item_id: "chief".into(),
					event_kind: "chief_turn_completed".into(),
					payload: serde_json::json!({"terminal":{"turn":{"status":status}}}).to_string(),
				},
			)
			.await
			.unwrap();
		let (events, _) = store.read_chief_transcript("chief".into(), None, 10).await.unwrap();
		let event = events.iter().find(|e| e.event_kind == "partial_output").unwrap();
		assert!(event.payload.len() <= 65536);
		let value: serde_json::Value = serde_json::from_str(&event.payload).unwrap();
		assert_eq!(value["truncated"], true);
		let saved = value["text"].as_str().unwrap();
		assert!(!saved.is_empty());
		assert!(text.starts_with(saved));
	}
}
