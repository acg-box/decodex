use super::*;

#[tokio::test]
async fn recovery_saves_large_result_without_duplicating_terminal_items() {
	let text = "界🙂\"\\\n\u{0001}".repeat(12_000);
	let turn = json!({"id":"opaque turn/1","status":"failed",
		"items":[{"type":"agentMessage","text":text}],
		"error":{"message":"Failure details ".repeat(8000),"code":"failed"}});
	let history = json!({"opaque thread/1":{"thread":{
		"id":"opaque thread/1","status":{"type":"idle"},"turns":[turn]
	}}});
	let (mut coordinator, _sent, _directory) = fixture_with_history(history).await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	coordinator.recover_persisted().await.unwrap();
	let work = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	assert_eq!(work.dispatch_state, decodex_database::ChiefDispatchState::Idle);
	assert!(work.active_turn_id.is_none());
	let events = coordinator.store.read_chief_work_events("chief".into(), 10).await.unwrap();
	let event = events.iter().find(|event| event.event_kind == "chief_turn_completed").unwrap();
	assert!(event.payload.len() <= 65536);
	let value: Value = serde_json::from_str(&event.payload).unwrap();
	assert_eq!(value["terminal"]["turn"]["id"], "opaque turn/1");
	assert_eq!(value["terminal"]["turn"]["status"], "failed");
	assert_eq!(value["terminal"]["detailsOmitted"], true);
	assert_eq!(value["terminal"]["turn"]["error"]["truncated"], true);
	assert!(value["terminal"]["turn"].get("items").is_none());
	assert_eq!(value["threadReadback"]["truncated"], true);
	let messages = value["threadReadback"]["assistantMessages"].as_array().unwrap();
	let retained = messages[0]["text"].as_str().unwrap();
	assert!(!retained.is_empty());
	assert!(text.starts_with(retained));
}
