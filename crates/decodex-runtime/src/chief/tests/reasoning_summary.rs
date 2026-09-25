use super::*;

#[tokio::test]
async fn reasoning_summary_stream_keeps_typed_items_and_excludes_raw_and_voice_delegation() {
	let (mut chief, _sent, _directory) = fixture().await;
	let work = chief.start_chief("chief", "Talk").await.unwrap();
	let event = |method: &str, mut params: Value| {
		params["threadId"] = json!(work.codex_thread_id);
		params["turnId"] = json!(work.active_turn_id);
		ServerEvent::Notification { method: method.into(), params }
	};
	chief
		.handle_event(event(
			"item/started",
			json!({"item":{"type":"reasoning","id":"typed","summary":[]}}),
		))
		.await
		.unwrap();
	chief
		.handle_event(event(
			"item/reasoning/textDelta",
			json!({"itemId":"typed","delta":"PRIVATE_RAW"}),
		))
		.await
		.unwrap();
	chief
		.handle_event(event(
			"item/reasoning/summaryTextDelta",
			json!({"itemId":"typed","summaryIndex":0,"delta":"Public summary."}),
		))
		.await
		.unwrap();
	chief.handle_event(event("item/started", json!({"item":{"type":"userMessage","id":"handoff","content":[{"type":"text","text":"<realtime_delegation><input>Voice request</input></realtime_delegation>","textElements":[]}]}}))).await.unwrap();
	chief
		.handle_event(event(
			"item/started",
			json!({"item":{"type":"reasoning","id":"voice","summary":[]}}),
		))
		.await
		.unwrap();
	chief
		.handle_event(event(
			"item/reasoning/summaryTextDelta",
			json!({"itemId":"voice","summaryIndex":0,"delta":"PRIVATE_VOICE"}),
		))
		.await
		.unwrap();
	chief
		.handle_event(event(
			"item/completed",
			json!({"item":{"type":"reasoning","id":"voice","summary":["PRIVATE_VOICE"]}}),
		))
		.await
		.unwrap();
	chief.handle_event(event("item/completed", json!({"item":{"type":"reasoning","id":"typed","summary":["Corrected public summary."],"content":["PRIVATE_RAW"]}}))).await.unwrap();
	chief
		.handle_event(event(
			"item/reasoning/summaryTextDelta",
			json!({"itemId":"typed","summaryIndex":0,"delta":"Late"}),
		))
		.await
		.unwrap();
	let live = chief.store.read_chief_output("chief".into()).await.unwrap();
	assert_eq!(live.len(), 1);
	assert_eq!(live[0].text, "Corrected public summary.");
	assert_eq!(live[0].kind, "reasoningSummary");
	complete(&mut chief, "chief").await;
	assert!(chief.store.read_chief_output("chief".into()).await.unwrap().is_empty());
}
