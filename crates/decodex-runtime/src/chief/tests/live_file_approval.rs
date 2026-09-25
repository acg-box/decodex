use super::*;

fn file_event(root: &ChiefWorkItem) -> Value {
	json!({"method":"item/started","params":{"threadId":root.codex_thread_id,"turnId":root.active_turn_id,"item":{"id":"patch","type":"fileChange","changes":[{"path":"/tmp/fixture","kind":{"type":"add"},"diff":format!("+{} REQUIRED FILE SUFFIX", "界".repeat(30000))}]}}})
}

#[tokio::test]
async fn live_file_approval_preserves_original_params_and_replays_exact_saved_evidence() {
	let (mut chief, _sent, directory) = fixture().await;
	let root = chief.start_chief("chief", "Coordinate").await.unwrap();
	let file = file_event(&root);
	let params = json!({"threadId":root.codex_thread_id,"turnId":root.active_turn_id,"itemId":"patch","reason":"Review"});
	let mut sent = attach_request_transport(
		&mut chief,
		json!({}),
		json!([file,{"id":91,"method":"item/fileChange/requestApproval","params":params}]),
	)
	.await;
	let id = chief.pending_requests[&RequestId::Number(91)];
	let saved = chief.store.get_chief_inbox_event(id).await.unwrap();
	let payload: Value = serde_json::from_str(&saved.payload).unwrap();
	assert_eq!(payload["params"], params);
	assert_eq!(payload["fileChange"], file["params"]["item"]);
	assert!(
		crate::chief_detail::saved_file_changes(&payload)
			.unwrap()
			.ends_with("REQUIRED FILE SUFFIX")
	);
	assert!(chief.pending_file_changes.get(chief.client.connection_identity(), &params).is_none());
	chief
		.handle_event(ServerEvent::Request {
			id: RequestId::Number(91),
			method: "item/fileChange/requestApproval".into(),
			params: params.clone(),
		})
		.await
		.unwrap();
	assert_eq!(chief.store.get_chief_inbox_event(id).await.unwrap().payload, saved.payload);
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let reopened = SqliteStore::open(&root.paths()).unwrap();
	assert_eq!(reopened.get_chief_inbox_event(id).await.unwrap().payload, saved.payload);
	chief.respond_pending_event(id, json!({"decision":"decline"})).await.unwrap();
	assert_eq!(sent.recv().await.unwrap(), json!({"id":91,"result":{"decision":"decline"}}));
	assert!(chief.respond_pending_event(id, json!({"decision":"accept"})).await.is_err());
}

#[tokio::test]
async fn failed_file_approval_write_keeps_evidence_until_commit() {
	let (mut chief, _sent, directory) = fixture().await;
	let root = chief.start_chief("chief", "Coordinate").await.unwrap();
	let file = file_event(&root);
	chief
		.handle_event(ServerEvent::Notification {
			method: "item/started".into(),
			params: file["params"].clone(),
		})
		.await
		.unwrap();
	let params =
		json!({"threadId":root.codex_thread_id,"turnId":root.active_turn_id,"itemId":"patch"});
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let db = rusqlite::Connection::open(root.paths().product_database_file()).unwrap();
	db.execute_batch("CREATE TRIGGER refuse_file_evidence BEFORE INSERT ON chief_request_payloads BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
	let request = || ServerEvent::Request {
		id: RequestId::Number(91),
		method: "item/fileChange/requestApproval".into(),
		params: params.clone(),
	};
	assert!(chief.handle_event(request()).await.is_err());
	assert!(chief.pending_file_changes.get(chief.client.connection_identity(), &params).is_some());
	db.execute_batch("DROP TRIGGER refuse_file_evidence").unwrap();
	chief.handle_event(request()).await.unwrap();
	assert!(chief.pending_file_changes.get(chief.client.connection_identity(), &params).is_none());
	let saved = chief
		.store
		.get_chief_inbox_event(chief.pending_requests[&RequestId::Number(91)])
		.await
		.unwrap();
	assert_eq!(
		serde_json::from_str::<Value>(&saved.payload).unwrap()["fileChange"],
		file["params"]["item"]
	);
}
