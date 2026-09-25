use super::*;

fn request(source: &str, method: &str, kind: &str) -> EnqueueChiefEvent {
	EnqueueChiefEvent {
		source_event_id: source.into(),
		work_item_id: "chief".into(),
		event_kind: kind.into(),
		payload: serde_json::json!({
			"id": 42, "method": method, "ownerThreadId": "thread",
			"params": {"threadId": "thread", "turnId": "turn", "itemId": "item",
				"command": "界".repeat(100_000)}
		})
		.to_string(),
	}
}

#[tokio::test]
async fn large_approval_details_are_atomic_exact_and_compact_in_scans() {
	let directory = tempdir().unwrap();
	let path = directory.path().join("requests.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	store.create_chief_work_item(item("chief", None)).await.unwrap();
	let input =
		request("native-request", "item/commandExecution/requestApproval", "permission_pending");
	store.with_connection(|connection| connection.execute_batch("CREATE TRIGGER reject_details BEFORE INSERT ON chief_request_payloads BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;").map_err(sqlite_error)).unwrap();
	assert!(store.enqueue_chief_event(input.clone()).await.is_err());
	let (events, _) = store.read_chief_transcript("chief".into(), None, 10).await.unwrap();
	assert!(events.is_empty(), "Failed detail writes must not leave compact-only requests");
	store
		.with_connection(|connection| {
			connection.execute_batch("DROP TRIGGER reject_details;").map_err(sqlite_error)
		})
		.unwrap();
	let saved = store.enqueue_chief_event(input.clone()).await.unwrap();
	assert_eq!(saved.payload, input.payload);
	let (events, _) = store.read_chief_transcript("chief".into(), None, 10).await.unwrap();
	assert_eq!(events.len(), 1);
	assert!(events[0].payload.len() < 1024);
	let compact: serde_json::Value = serde_json::from_str(&events[0].payload).unwrap();
	assert_eq!(compact["params"]["itemId"], "item");
	assert_eq!(compact["detailsStored"], true);
	assert!(compact["params"]["command"].is_null());
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	assert_eq!(store.get_chief_inbox_event(saved.id).await.unwrap().payload, input.payload);
	assert_eq!(store.enqueue_chief_event(input.clone()).await.unwrap().id, saved.id);
	let mut changed = input;
	changed.payload = changed.payload.replace("界", "文");
	assert!(matches!(
		store.enqueue_chief_event(changed).await,
		Err(StoreError::IdempotencyConflict)
	));
	let (events, _) = store.read_chief_transcript("chief".into(), None, 10).await.unwrap();
	assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn large_approval_bounds_do_not_expand_other_inbox_events() {
	let directory = tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("requests.sqlite3")).unwrap();
	store.create_chief_work_item(item("chief", None)).await.unwrap();
	for (method, kind) in [
		("item/commandExecution/requestApproval", "permission_pending"),
		("item/fileChange/requestApproval", "permission_pending"),
		("item/permissions/requestApproval", "permission_pending"),
		("mcpServer/elicitation/request", "server_request_pending"),
	] {
		let input = request(method, method, kind);
		assert_eq!(store.enqueue_chief_event(input.clone()).await.unwrap().payload, input.payload);
	}
	for (method, kind) in [
		("item/commandExecution/requestApproval", "user_message"),
		("item/tool/call", "server_request_pending"),
		("item/tool/requestUserInput", "user_input_pending"),
		("mcpServer/elicitation/request", "permission_pending"),
	] {
		assert!(store.enqueue_chief_event(request("rejected", method, kind)).await.is_err());
	}
	let mut input =
		request("oversized", "item/commandExecution/requestApproval", "permission_pending");
	input.payload = serde_json::json!({"id":42,"method":"item/commandExecution/requestApproval","params":{"command":"界".repeat(3_000_000)}}).to_string();
	assert!(store.enqueue_chief_event(input).await.is_err());
}
