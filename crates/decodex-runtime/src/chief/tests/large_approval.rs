use super::*;

#[tokio::test]
async fn large_user_approval_keeps_complete_request_without_breaking_the_coordinator() {
	let (mut coordinator, _sent, directory) = fixture().await;
	let root = coordinator.start_chief("chief", "Coordinate").await.unwrap();
	let command = format!("true # {} REQUIRED ACTION SUFFIX", "界".repeat(100_000));
	let params = json!({"threadId":root.codex_thread_id,"turnId":root.active_turn_id,
		"itemId":"large-command","command":command,"cwd":"/tmp",
		"availableDecisions":["accept","decline"]});
	coordinator
		.handle_event(ServerEvent::Request {
			id: RequestId::Number(71),
			method: "item/commandExecution/requestApproval".into(),
			params: params.clone(),
		})
		.await
		.expect("native-sized user approval must remain pending without failing the coordinator");
	let pending = coordinator.store.list_pending_chief_events(100).await.unwrap();
	let request = pending.iter().find(|event| event.event_kind == "permission_pending").unwrap();
	assert!(request.payload.len() < 4096, "inbox scans must not expand complete actions");
	let saved = coordinator.store.get_chief_inbox_event(request.id).await.unwrap();
	let payload: Value = serde_json::from_str(&saved.payload).unwrap();
	assert_eq!(payload["params"], params);
	let input = EnqueueChiefEvent {
		source_event_id: saved.source_event_id.clone(),
		work_item_id: saved.work_item_id.clone(),
		event_kind: saved.event_kind.clone(),
		payload: saved.payload.clone(),
	};
	assert_eq!(coordinator.store.enqueue_chief_event(input.clone()).await.unwrap().id, saved.id);
	let mut changed = input;
	changed.payload = saved.payload.replace("REQUIRED ACTION SUFFIX", "CHANGED ACTION SUFFIX");
	assert!(coordinator.store.enqueue_chief_event(changed).await.is_err());
	let mcp = json!({"threadId":root.codex_thread_id,"turnId":root.active_turn_id,
		"mode":"form","serverName":"fixture","message":"Approve the complete tool action",
		"requestedSchema":{"type":"object","properties":{}},
		"_meta":{"codex_approval_kind":"tool_call","tool_params":{"command":command}}});
	coordinator
		.handle_event(ServerEvent::Request {
			id: RequestId::Number(72),
			method: "mcpServer/elicitation/request".into(),
			params: mcp.clone(),
		})
		.await
		.unwrap();
	let pending = coordinator.store.list_pending_chief_events(100).await.unwrap();
	let mcp_event =
		pending.iter().find(|event| event.event_kind == "server_request_pending").unwrap();
	assert!(mcp_event.payload.len() < 4096);
	let mcp_id = mcp_event.id;
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	drop(coordinator);
	let reopened = SqliteStore::open(&root.paths()).unwrap();
	assert_eq!(reopened.get_chief_inbox_event(saved.id).await.unwrap().payload, saved.payload);
	let mcp_payload: Value =
		serde_json::from_str(&reopened.get_chief_inbox_event(mcp_id).await.unwrap().payload)
			.unwrap();
	assert_eq!(mcp_payload["params"], mcp);
}
