use crate::agent::tests::{self, ClientError, Value, result_messages};

#[test]
fn completion_summary_excludes_nonfinal_and_unscoped_items() {
	let turn = serde_json::json!({"status":"completed","itemsView":"summary","items":[
		{"type":"agentMessage","id":"valid","text":"Legacy final"},
		{"type":"agentMessage","id":"comment","phase":"commentary","text":"Progress"},
		{"type":"agentMessage","id":"empty","phase":"final_answer","text":" "},
		{"type":"agentMessage","phase":"final_answer","text":"Missing identity"}
	]});

	assert_eq!(result_messages::completion_summary(&turn).unwrap()["id"], "valid");

	for (field, value) in [("status", "failed"), ("itemsView", "notLoaded")] {
		let mut invalid = turn.clone();

		invalid[field] = serde_json::json!(value);

		assert!(result_messages::completion_summary(&invalid).is_none());
	}
}

#[tokio::test]
async fn paginated_recovery_retains_exact_worker_output_without_full_thread_hydration() {
	let history = serde_json::json!({"opaque thread/1":{"thread":{
		"id":"opaque thread/1","historyMode":"paginated","status":{"type":"idle"},
		"turns":[{"id":"opaque turn/1","status":"completed",
		"startedAt":1_700_000_000,"completedAt":1_700_000_125,"durationMs":125_000,"items":[
			{"id":"answer","type":"agentMessage","text":"Recovered result","phase":"final_answer"}
		]}]
	}}});
	let (mut coordinator, mut sent, _directory) = tests::fixture_with_history(history).await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	coordinator.recover_persisted().await.unwrap();

	let work = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Idle);

	let events = coordinator.store.read_agent_work_events("agent".into(), 10).await.unwrap();
	let event = events.iter().find(|event| event.event_kind == "agent_turn_completed").unwrap();
	let payload: Value = serde_json::from_str(&event.payload).unwrap();

	assert_eq!(payload["threadReadback"]["assistantMessages"][0]["text"], "Recovered result");
	assert_eq!(payload["threadReadback"]["exactTurnReadback"], true);
	assert_eq!(payload["terminal"]["turn"]["startedAt"], 1_700_000_000);
	assert_eq!(payload["terminal"]["turn"]["completedAt"], 1_700_000_125);
	assert_eq!(payload["terminal"]["turn"]["durationMs"], 125_000);

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start");
		assert_ne!(request["params"]["includeTurns"], true);

		if request["method"] == "thread/resume" {
			assert_eq!(request["params"]["excludeTurns"], true);
		}
	}
}

#[tokio::test]
async fn recovery_saves_large_result_without_duplicating_terminal_items() {
	for with_id in [false, true] {
		let text = "界🙂\"\\\n\u{0001}".repeat(12_000);
		let mut item = serde_json::json!({"type":"agentMessage","text":text});

		if with_id {
			item["id"] = serde_json::json!("large-answer");
		}

		let turn = serde_json::json!({"id":"opaque turn/1","status":"failed",
			"items":[item],
			"error":{"message":"Failure details ".repeat(8_000),"code":"failed"}});
		let history = serde_json::json!({"opaque thread/1":{"thread":{
			"id":"opaque thread/1","status":{"type":"idle"},"turns":[turn]
		}}});
		let (mut coordinator, _sent, _directory) = tests::fixture_with_history(history).await;

		coordinator.start_agent("agent", "Coordinate").await.unwrap();
		coordinator.recover_persisted().await.unwrap();

		let work = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();

		assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Idle);
		assert!(work.active_turn_id.is_none());

		let events = coordinator.store.read_agent_work_events("agent".into(), 10).await.unwrap();
		let event = events.iter().find(|event| event.event_kind == "agent_turn_completed").unwrap();

		assert!(event.payload.len() <= 65_536);

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
}

#[tokio::test]
async fn completion_summary_repairs_missing_final_output_without_claiming_full_readback() {
	for readback in [
		Err(ClientError::Closed),
		Ok(
			serde_json::json!({"thread":{"id":"foreign","turns":[{"id":"opaque turn/1","items":[{"type":"agentMessage","id":"foreign-answer","text":"Foreign output"}]}]}}),
		),
	] {
		let (mut coordinator, _sent, _directory) = tests::fixture().await;

		coordinator.start_agent("agent", "Coordinate").await.unwrap();
		coordinator.observe_live_text("item/agentMessage/delta", &serde_json::json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","itemId":"answer","delta":"Partial"})).await.unwrap();

		let params = serde_json::json!({"threadId":"opaque thread/1","turn":{"id":"opaque turn/1","status":"completed","itemsView":"summary","items":[{"type":"agentMessage","id":"answer","phase":"final_answer","text":"Complete answer"}]}});

		coordinator.record_terminal(params.clone(), readback, true).await.unwrap();
		// A repeated terminal cannot append or replace output for a finished turn.
		coordinator.record_terminal(params, Err(ClientError::Closed), true).await.unwrap();

		let output = coordinator.store.read_agent_output("agent".into()).await.unwrap();

		assert!(output.is_empty(), "terminal event replaces transient output");

		let events = coordinator.store.read_agent_work_events("agent".into(), 20).await.unwrap();
		let completed: Vec<_> =
			events.iter().filter(|e| e.event_kind == "agent_turn_completed").collect();

		assert_eq!(completed.len(), 1);

		let payload: Value = serde_json::from_str(&completed[0].payload).unwrap();

		assert_eq!(payload["threadReadback"]["threadId"], "opaque thread/1");
		assert_eq!(payload["threadReadback"]["turnId"], "opaque turn/1");
		assert_eq!(payload["threadReadback"]["exactTurnReadback"], false);
		assert_eq!(payload["threadReadback"]["assistantMessagesSource"], "turnCompletionSummary");
		assert_eq!(payload["threadReadback"]["assistantMessages"][0]["text"], "Complete answer");
	}
}
