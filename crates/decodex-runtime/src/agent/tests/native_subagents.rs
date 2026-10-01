use crate::{agent::tests::*, native_agents};
use decodex_core::DecodexRoot;
use decodex_protocol::NativeAgentsResult;

fn child(thread: &str, parent: &str) -> Value {
	serde_json::json!({"thread":{"id":thread,"parentThreadId":parent,"source":{"subAgent":{"thread_spawn":{"parent_thread_id":parent}}},"canAcceptDirectInput":false}})
}

#[tokio::test]
async fn native_child_reconnect_and_peer_resolution_require_exact_child_request() {
	let (mut agent, mut sent, _directory) =
		fixture_with_history(serde_json::json!({"child":child("child","opaque thread/1")})).await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	let params = serde_json::json!({"threadId":"child","turnId":"child-turn","questions":[]});
	let request = || ServerEvent::Request {
		id: RequestId::Number(71),
		method: "item/tool/requestUserInput".into(),
		params: params.clone(),
	};

	agent.handle_event(request()).await.unwrap();

	let old = agent.pending_requests[&RequestId::Number(71)];
	let mut reconnected =
		AgentCoordinator::new(agent.store.clone(), agent.client.clone(), agent.config.clone())
			.unwrap();

	while sent.try_recv().is_ok() {}

	assert!(
		reconnected.respond_pending_event(old, serde_json::json!({"answers":{}})).await.is_err()
	);
	assert!(sent.try_recv().is_err());

	reconnected.handle_event(request()).await.unwrap();

	let new = reconnected.pending_requests[&RequestId::Number(71)];

	assert_ne!(new, old);

	for thread in ["opaque thread/1", "child"] {
		reconnected
			.handle_event(ServerEvent::Notification {
				method: "serverRequest/resolved".into(),
				params: serde_json::json!({"threadId":thread,"requestId":71}),
			})
			.await
			.unwrap();

		assert_eq!(
			reconnected.pending_requests.contains_key(&RequestId::Number(71)),
			thread != "child"
		);
	}

	assert!(
		reconnected.respond_pending_event(new, serde_json::json!({"answers":{}})).await.is_err()
	);
	assert_eq!(
		agent.store.get_agent_inbox_event(new).await.unwrap().disposition,
		Some(AgentDisposition::Resolved)
	);
}

#[tokio::test]
async fn nested_native_approval_keeps_child_identity_and_resolves_once() {
	let history = serde_json::json!({"child":child("child","opaque thread/1"),"grandchild":child("grandchild","child")});
	let (mut agent, _old_sent, directory) = fixture_with_history(history.clone()).await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	let params = serde_json::json!({"threadId":"grandchild","turnId":"child-turn","itemId":"command","command":"pwd"});
	let mut sent = attach_request_transport(
		&mut agent,
		history,
		serde_json::json!({"id":71,"method":"item/commandExecution/requestApproval","params":params}),
	)
	.await;
	let event_id = agent.pending_requests[&RequestId::Number(71)];
	let event = agent.store.get_agent_inbox_event(event_id).await.unwrap();

	assert_eq!(event.work_item_id, "agent");

	let payload: Value = serde_json::from_str(&event.payload).unwrap();

	assert_eq!(payload["params"], params);
	assert_eq!(payload["ownerThreadId"], "opaque thread/1");

	while sent.try_recv().is_ok() {}

	agent.respond_pending_event(event_id, serde_json::json!({"decision":"decline"})).await.unwrap();

	let mut replies = Vec::new();

	while let Ok(request) = sent.try_recv() {
		if request.get("method").is_none() {
			replies.push(request);
		} else {
			assert_eq!(request["method"], "thread/read");
		}
	}

	assert_eq!(replies, vec![serde_json::json!({"id":71,"result":{"decision":"decline"}})]);
	assert!(
		agent
			.respond_pending_event(event_id, serde_json::json!({"decision":"accept"}))
			.await
			.is_err()
	);

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
	let reopened = SqliteStore::open(&root.paths()).unwrap();

	assert!(reopened.get_agent_inbox_event(event_id).await.unwrap().disposition.is_some());
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().active_turn_id.as_deref(),
		Some("opaque turn/1")
	);
}

#[tokio::test]
async fn unowned_fork_mismatched_readback_and_cyclic_children_do_not_gain_authority() {
	for native in [
		child("child", "unowned"),
		child("other", "opaque thread/1"),
		child("child", "child"),
		serde_json::json!({"thread":{"id":"child","parentThreadId":"opaque thread/1","source":"appServer"}}),
		serde_json::json!({"thread":{"id":"child","parentThreadId":"opaque thread/1","source":{"subAgent":{"thread_spawn":{"parent_thread_id":"different"}}}}}),
	] {
		let (mut agent, mut sent, _directory) =
			fixture_with_history(serde_json::json!({"child":native})).await;

		agent.start_agent("agent", "Coordinate").await.unwrap();

		while sent.try_recv().is_ok() {}

		assert!(
			agent
				.handle_event(ServerEvent::Request {
					id: RequestId::Number(71),
					method: "item/commandExecution/requestApproval".into(),
					params: serde_json::json!({"threadId":"child","turnId":"turn"})
				})
				.await
				.is_err()
		);
		assert!(agent.pending_requests.is_empty());

		while let Ok(request) = sent.try_recv() {
			assert_eq!(request["method"], "thread/read");
		}
	}
}

#[tokio::test]
async fn native_children_do_not_inherit_agent_management_tools() {
	let (mut agent, mut sent, _directory) =
		fixture_with_history(serde_json::json!({"child":child("child","opaque thread/1")})).await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	agent.handle_event(ServerEvent::Request {id:RequestId::Number(71),method:"item/tool/call".into(),params:serde_json::json!({"threadId":"child","turnId":"opaque turn/1","tool":"agent_create_work","arguments":{"id":"forbidden","prompt":"execute"}})}).await.unwrap();

	assert!(agent.pending_requests.is_empty());
	assert!(agent.store.get_agent_work_item("forbidden".into()).await.is_err());

	let mut refused = false;

	while let Ok(request) = sent.try_recv() {
		if request.get("method").is_none() {
			assert_eq!(request["id"], 71);
			assert_eq!(request["result"]["success"], false);

			refused = true;
		} else {
			assert_eq!(request["method"], "thread/read");
		}
	}

	assert!(refused);
}

#[tokio::test]
async fn native_agent_inspection_requires_exact_ancestry_and_never_starts_work() {
	let mut native = child("child", "opaque thread/1");

	native["thread"]["turns"] = serde_json::json!([]);

	let (mut agent, mut sent, _directory) = fixture_with_history(
		serde_json::json!({"child":native,"foreign":child("foreign","unrelated")}),
	)
	.await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let result =
		native_agents::read(&agent.store, &agent.client, "agent", Some("child"), None).await;

	assert!(matches!(result, NativeAgentsResult::Conversation { can_input: false, .. }));
	assert!(matches!(
		native_agents::read(&agent.store, &agent.client, "agent", Some("foreign"), None).await,
		decodex_protocol::NativeAgentsResult::Unavailable
	));

	while let Ok(request) = sent.try_recv() {
		assert_eq!(request["method"], "thread/read");
	}

	assert_eq!(agent.store.list_agent_work_items().await.unwrap().len(), 1);
}
