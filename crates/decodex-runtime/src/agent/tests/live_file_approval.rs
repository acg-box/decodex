use rusqlite::Connection;

use crate::{agent::tests::*, agent_detail};
use decodex_core::DecodexRoot;

fn file_event(root: &AgentWorkItem) -> Value {
	serde_json::json!({"method":"item/started","params":{"threadId":root.codex_thread_id,"turnId":root.active_turn_id,"item":{"id":"patch","type":"fileChange","changes":[{"path":"/tmp/fixture","kind":{"type":"add"},"diff":format!("+{} REQUIRED FILE SUFFIX", "界".repeat(30_000))}]}}})
}

#[tokio::test]
async fn live_file_approval_preserves_original_params_and_replays_exact_saved_evidence() {
	let (mut agent, _sent, directory) = fixture().await;
	let root = agent.start_agent("agent", "Coordinate").await.unwrap();
	let file = file_event(&root);
	let params = serde_json::json!({"threadId":root.codex_thread_id,"turnId":root.active_turn_id,"itemId":"patch","reason":"Review"});
	let mut sent = attach_request_transport(
		&mut agent,
		serde_json::json!({}),
		serde_json::json!([file,{"id":91,"method":"item/fileChange/requestApproval","params":params}]),
	)
	.await;
	let id = agent.pending_requests[&RequestId::Number(91)];
	let saved = agent.store.get_agent_inbox_event(id).await.unwrap();
	let payload: Value = serde_json::from_str(&saved.payload).unwrap();

	assert_eq!(payload["params"], params);
	assert_eq!(payload["fileChange"], file["params"]["item"]);
	assert!(agent_detail::saved_file_changes(&payload).unwrap().ends_with("REQUIRED FILE SUFFIX"));
	assert!(agent.pending_file_changes.get(agent.client.connection_identity(), &params).is_none());

	agent
		.handle_event(ServerEvent::Request {
			id: RequestId::Number(91),
			method: "item/fileChange/requestApproval".into(),
			params: params.clone(),
		})
		.await
		.unwrap();

	assert_eq!(agent.store.get_agent_inbox_event(id).await.unwrap().payload, saved.payload);

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
	let reopened = SqliteStore::open(&root.paths()).unwrap();

	assert_eq!(reopened.get_agent_inbox_event(id).await.unwrap().payload, saved.payload);

	agent.respond_pending_event(id, serde_json::json!({"decision":"decline"})).await.unwrap();

	assert_eq!(
		sent.recv().await.unwrap(),
		serde_json::json!({"id":91,"result":{"decision":"decline"}})
	);
	assert!(
		agent.respond_pending_event(id, serde_json::json!({"decision":"accept"})).await.is_err()
	);
}

#[tokio::test]
async fn failed_file_approval_write_keeps_evidence_until_commit() {
	let (mut agent, _sent, directory) = fixture().await;
	let root = agent.start_agent("agent", "Coordinate").await.unwrap();
	let file = file_event(&root);

	agent
		.handle_event(ServerEvent::Notification {
			method: "item/started".into(),
			params: file["params"].clone(),
		})
		.await
		.unwrap();

	let params = serde_json::json!({"threadId":root.codex_thread_id,"turnId":root.active_turn_id,"itemId":"patch"});
	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
	let db = Connection::open(root.paths().product_database_file()).unwrap();

	db.execute_batch("CREATE TRIGGER refuse_file_evidence BEFORE INSERT ON agent_request_payloads BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();

	let request = || ServerEvent::Request {
		id: RequestId::Number(91),
		method: "item/fileChange/requestApproval".into(),
		params: params.clone(),
	};

	assert!(agent.handle_event(request()).await.is_err());
	assert!(agent.pending_file_changes.get(agent.client.connection_identity(), &params).is_some());

	db.execute_batch("DROP TRIGGER refuse_file_evidence").unwrap();
	agent.handle_event(request()).await.unwrap();

	assert!(agent.pending_file_changes.get(agent.client.connection_identity(), &params).is_none());

	let saved = agent
		.store
		.get_agent_inbox_event(agent.pending_requests[&RequestId::Number(91)])
		.await
		.unwrap();

	assert_eq!(
		serde_json::from_str::<Value>(&saved.payload).unwrap()["fileChange"],
		file["params"]["item"]
	);
}
