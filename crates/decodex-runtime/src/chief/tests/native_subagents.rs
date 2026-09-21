use super::*;

fn child(thread: &str, parent: &str) -> Value {
	json!({"thread":{"id":thread,"parentThreadId":parent,"source":{"subAgent":{"thread_spawn":{"parent_thread_id":parent}}},"canAcceptDirectInput":false}})
}

#[tokio::test]
async fn native_child_reconnect_and_peer_resolution_require_exact_child_request() {
	let (mut chief, mut sent, _directory) =
		fixture_with_history(json!({"child":child("child","opaque thread/1")})).await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	let params = json!({"threadId":"child","turnId":"child-turn","questions":[]});
	let request = || ServerEvent::Request {
		id: RequestId::Number(71),
		method: "item/tool/requestUserInput".into(),
		params: params.clone(),
	};
	chief.handle_event(request()).await.unwrap();
	let old = chief.pending_requests[&RequestId::Number(71)];
	let mut reconnected =
		ChiefCoordinator::new(chief.store.clone(), chief.client.clone(), chief.config.clone())
			.unwrap();
	while sent.try_recv().is_ok() {}
	assert!(reconnected.respond_pending_event(old, json!({"answers":{}})).await.is_err());
	assert!(sent.try_recv().is_err());
	reconnected.handle_event(request()).await.unwrap();
	let new = reconnected.pending_requests[&RequestId::Number(71)];
	assert_ne!(new, old);
	for thread in ["opaque thread/1", "child"] {
		reconnected
			.handle_event(ServerEvent::Notification {
				method: "serverRequest/resolved".into(),
				params: json!({"threadId":thread,"requestId":71}),
			})
			.await
			.unwrap();
		assert_eq!(
			reconnected.pending_requests.contains_key(&RequestId::Number(71)),
			thread != "child"
		);
	}
	assert!(reconnected.respond_pending_event(new, json!({"answers":{}})).await.is_err());
	assert_eq!(
		chief.store.get_chief_inbox_event(new).await.unwrap().disposition,
		Some(ChiefDisposition::Resolved)
	);
}

#[tokio::test]
async fn nested_native_approval_keeps_child_identity_and_resolves_once() {
	let history =
		json!({"child":child("child","opaque thread/1"),"grandchild":child("grandchild","child")});
	let (mut chief, _old_sent, directory) = fixture_with_history(history.clone()).await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	let params =
		json!({"threadId":"grandchild","turnId":"child-turn","itemId":"command","command":"pwd"});
	let mut sent = attach_request_transport(
		&mut chief,
		history,
		json!({"id":71,"method":"item/commandExecution/requestApproval","params":params}),
	)
	.await;
	let event_id = chief.pending_requests[&RequestId::Number(71)];
	let event = chief.store.get_chief_inbox_event(event_id).await.unwrap();
	assert_eq!(event.work_item_id, "chief");
	let payload: Value = serde_json::from_str(&event.payload).unwrap();
	assert_eq!(payload["params"], params);
	assert_eq!(payload["ownerThreadId"], "opaque thread/1");
	while sent.try_recv().is_ok() {}
	chief.respond_pending_event(event_id, json!({"decision":"decline"})).await.unwrap();
	let mut replies = Vec::new();
	while let Ok(request) = sent.try_recv() {
		if request.get("method").is_none() {
			replies.push(request);
		} else {
			assert_eq!(request["method"], "thread/read");
		}
	}
	assert_eq!(replies, vec![json!({"id":71,"result":{"decision":"decline"}})]);
	assert!(chief.respond_pending_event(event_id, json!({"decision":"accept"})).await.is_err());
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let reopened = SqliteStore::open(&root.paths()).unwrap();
	assert!(reopened.get_chief_inbox_event(event_id).await.unwrap().disposition.is_some());
	assert_eq!(
		chief.store.get_chief_work_item("chief".into()).await.unwrap().active_turn_id.as_deref(),
		Some("opaque turn/1")
	);
}

#[tokio::test]
async fn unowned_fork_mismatched_readback_and_cyclic_children_do_not_gain_authority() {
	for native in [
		child("child", "unowned"),
		child("other", "opaque thread/1"),
		child("child", "child"),
		json!({"thread":{"id":"child","parentThreadId":"opaque thread/1","source":"appServer"}}),
		json!({"thread":{"id":"child","parentThreadId":"opaque thread/1","source":{"subAgent":{"thread_spawn":{"parent_thread_id":"different"}}}}}),
	] {
		let (mut chief, mut sent, _directory) = fixture_with_history(json!({"child":native})).await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		while sent.try_recv().is_ok() {}
		assert!(
			chief
				.handle_event(ServerEvent::Request {
					id: RequestId::Number(71),
					method: "item/commandExecution/requestApproval".into(),
					params: json!({"threadId":"child","turnId":"turn"})
				})
				.await
				.is_err()
		);
		assert!(chief.pending_requests.is_empty());
		while let Ok(request) = sent.try_recv() {
			assert_eq!(request["method"], "thread/read");
		}
	}
}

#[tokio::test]
async fn native_children_do_not_inherit_chief_management_tools() {
	let (mut chief, mut sent, _directory) =
		fixture_with_history(json!({"child":child("child","opaque thread/1")})).await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	chief.handle_event(ServerEvent::Request {id:RequestId::Number(71),method:"item/tool/call".into(),params:json!({"threadId":"child","turnId":"opaque turn/1","tool":"chief_create_work","arguments":{"id":"forbidden","prompt":"execute"}})}).await.unwrap();
	assert!(chief.pending_requests.is_empty());
	assert!(chief.store.get_chief_work_item("forbidden".into()).await.is_err());
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
