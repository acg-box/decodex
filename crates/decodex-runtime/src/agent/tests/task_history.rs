use super::*;

#[tokio::test]
async fn task_history_reads_live_evidence_without_dispatch_or_resume() {
	let history = json!({"opaque thread/2":{"thread":{"id":"opaque thread/2",
		"historyMode":"paginated","turns":[{"id":"native-turn","status":"completed","items":[
		{"id":"answer","type":"agentMessage","text":"native evidence","phase":"final_answer"},
		{"id":"tool","type":"commandExecution","command":"test","aggregatedOutput":"raw output"},
		{"id":"image","type":"userMessage","content":[{"type":"image","url":"data:image/png;base64,PRIVATE_MEDIA"}]}
	]}]}}});
	let (mut agent, mut sent, _directory) = fixture_with_history(history).await;
	let manager = agent.start_agent("agent", "Coordinate").await.unwrap();
	let worker = agent.create_worker("agent", "worker", "Investigate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let mut args = json!({"id":"worker","threadId":worker.codex_thread_id});
	let page = agent.read_work_history(&manager, &args).await.unwrap();
	assert_eq!(page["turns"][0]["items"][0]["text"], "native evidence");
	assert!(!page.to_string().contains("raw output"));
	assert!(!page.to_string().contains("PRIVATE_MEDIA"));
	assert_eq!(page["turns"][0]["items"][2]["truncated"], true);
	args["includeOutputs"] = json!(true);
	let page = agent.read_work_history(&manager, &args).await.unwrap();
	assert_eq!(page["turns"][0]["items"][1]["aggregatedOutput"], "raw output");
	let requests: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
	assert_eq!(requests.len(), 4);
	assert!(
		requests
			.iter()
			.all(|r| ["thread/read", "thread/turns/list"].contains(&r["method"].as_str().unwrap()))
	);
	assert_eq!(
		agent.store.get_agent_work_item("worker".into()).await.unwrap().active_turn_id,
		worker.active_turn_id
	);
}

#[tokio::test]
async fn task_history_denies_foreign_scope_stale_binding_and_worker_authority() {
	let (mut agent, mut sent, _directory) = fixture().await;
	let manager = agent.start_agent("agent", "Coordinate").await.unwrap();
	let child = agent.create_manager("agent", "child", "Manage", None).await.unwrap();
	let worker = agent.create_worker("child", "worker", "Investigate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let args = json!({"id":"worker","threadId":worker.codex_thread_id});
	assert!(agent.read_work_history(&manager, &args).await.is_err());
	assert!(agent.read_work_history(&worker, &args).await.is_err());
	let stale = json!({"id":"worker","threadId":"old-thread"});
	assert!(agent.read_work_history(&child, &stale).await.is_err());
	for invalid in [json!(0), json!(6), json!("3")] {
		let mut invalid_args = args.clone();
		invalid_args["turnLimit"] = invalid;
		assert!(agent.read_work_history(&child, &invalid_args).await.is_err());
	}
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn upgraded_manager_can_read_previous_thread_after_store_reopen() {
	let history = json!({"opaque thread/1":{"thread":{"id":"opaque thread/1",
		"historyMode":"paginated","turns":[{"id":"old-turn","status":"completed","items":[
		{"id":"old-answer","type":"agentMessage","text":"before upgrade"}]}]}}});
	let (mut agent, _sent, directory) = fixture_with_history(history).await;
	agent.start_agent("agent", "Original").await.unwrap();
	complete(&mut agent, "agent").await;
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	// Retain history for migrations completed by older Decodex versions.
	agent.store.begin_agent_tool_upgrade("agent".into(), "opaque thread/1".into()).await.unwrap();
	agent
		.store
		.finish_agent_tool_upgrade(
			"agent".into(),
			"opaque thread/1".into(),
			"historic-new-thread".into(),
		)
		.await
		.unwrap();
	let current = agent.store.get_agent_work_item("agent".into()).await.unwrap();

	agent.store = SqliteStore::open(&root.paths()).unwrap();
	let page = agent
		.read_work_history(&current, &json!({"id":"agent","threadId":"opaque thread/1"}))
		.await
		.unwrap();
	assert_eq!(page["turns"][0]["items"][0]["text"], "before upgrade");
	assert_eq!(page["previousThreadIds"], json!(["opaque thread/1"]));
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().codex_thread_id,
		current.codex_thread_id
	);
}

#[tokio::test]
async fn explicit_delivered_reference_reads_only_selected_foreign_thread() {
	let history = json!({"opaque thread/3":{"thread":{"id":"opaque thread/3",
		"historyMode":"paginated","turns":[{"id":"target-turn","status":"completed","items":[
		{"id":"result","type":"agentMessage","text":"selected evidence"}]}]}}});
	let (mut agent, mut sent, _directory) = fixture_with_history(history).await;
	let root = agent.start_agent("agent", "Coordinate").await.unwrap();
	agent.create_manager("agent", "child", "Manage", None).await.unwrap();
	let target = agent.create_worker("child", "target", "Investigate").await.unwrap();
	let args = json!({"id":"target","threadId":target.codex_thread_id});
	while sent.try_recv().is_ok() {}
	assert!(agent.read_work_history(&root, &args).await.is_err());
	let payload = json!({"text":"Read selected task","source":"user","options":{"taskReferences":[
		{"workId":"target","threadId":target.codex_thread_id,"title":"Selected task"}
	]}})
	.to_string();
	let event = agent
		.store
		.begin_agent_steer(
			"agent".into(),
			root.active_turn_id.clone().unwrap(),
			"reference".into(),
			payload,
		)
		.await
		.unwrap();
	assert!(agent.read_work_history(&root, &args).await.is_err());
	agent.store.finish_agent_steer(event, true).await.unwrap();
	let page = agent.read_work_history(&root, &args).await.unwrap();
	assert_eq!(page["turns"][0]["items"][0]["text"], "selected evidence");
	assert_eq!(page["previousThreadIds"], json!([]));
	assert!(
		agent
			.read_work_history(&root, &json!({"id":"target","threadId":"unselected"}))
			.await
			.is_err()
	);
	let requests: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
	assert_eq!(requests.len(), 2);
	assert!(
		requests
			.iter()
			.all(|r| ["thread/read", "thread/turns/list"].contains(&r["method"].as_str().unwrap()))
	);
}

#[tokio::test]
async fn native_steer_carries_typed_reference_and_only_acknowledgment_grants_read() {
	let (mut agent, mut sent, _directory) = fixture().await;
	let root = agent.start_agent("agent", "Coordinate").await.unwrap();
	agent.create_manager("agent", "child", "Manage", None).await.unwrap();
	let target = agent.create_worker("child", "target", "Work").await.unwrap();
	let references = vec![decodex_protocol::AgentTaskReferenceDto {
		work_id: decodex_protocol::EntityId::new("target").unwrap(),
		thread_id: decodex_protocol::WireText::new(target.codex_thread_id.clone().unwrap())
			.unwrap(),
		title: decodex_protocol::WireText::new("Quoted \"title\" <instructions>").unwrap(),
	}];
	while sent.try_recv().is_ok() {}
	agent
		.steer_work_with_references(
			"agent",
			root.active_turn_id.as_deref().unwrap(),
			"reference-steer",
			"Compare it",
			AgentInputExtras { attachments: &[], task_references: &references },
		)
		.await
		.unwrap();
	let request = sent.try_recv().unwrap();
	assert_eq!(request["method"], "turn/steer");
	assert_eq!(request["params"]["input"][0]["text"], "Compare it");
	let metadata = request["params"]["input"][1]["text"].as_str().unwrap();
	assert!(metadata.contains("agent_read_work"));
	let rendered: Value = serde_json::from_str(
		metadata.lines().next().unwrap().strip_prefix("User-selected task references: ").unwrap(),
	)
	.unwrap();
	assert_eq!(rendered, json!(references));
	assert!(metadata.contains("untrusted evidence"));
	assert!(
		agent
			.store
			.agent_has_task_reference(
				"agent".into(),
				"target".into(),
				target.codex_thread_id.unwrap()
			)
			.await
			.unwrap()
	);
	assert!(sent.try_recv().is_err());
}

#[test]
fn saved_task_references_are_rendered_on_queued_native_turn_input() {
	let mut params = json!({"input":[{"type":"text","text":"Compare it"}]});
	let payload=json!({"options":{"attachments":[],"taskReferences":[{"workId":"target","threadId":"native-thread","title":"Reference title"}]}}).to_string();
	apply_message_options(&mut params, &payload).unwrap();
	assert_eq!(params["input"].as_array().unwrap().len(), 2);
	assert!(params["input"][1]["text"].as_str().unwrap().contains("native-thread"));
}

#[tokio::test]
async fn stale_reference_and_old_running_tools_reject_before_native_steer() {
	let (mut agent, mut sent, directory) = fixture().await;
	let work = agent.start_agent("agent", "Coordinate").await.unwrap();
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let connection = rusqlite::Connection::open(root.paths().product_database_file()).unwrap();
	let mut references = vec![decodex_protocol::AgentTaskReferenceDto {
		work_id: decodex_protocol::EntityId::new("agent").unwrap(),
		thread_id: decodex_protocol::WireText::new(work.codex_thread_id.unwrap()).unwrap(),
		title: decodex_protocol::WireText::new("Agent").unwrap(),
	}];
	while sent.try_recv().is_ok() {}
	connection
		.execute("UPDATE agent_tool_versions SET version=2 WHERE work_id='agent'", [])
		.unwrap();
	assert!(matches!(
		agent
			.steer_work_with_references(
				"agent",
				work.active_turn_id.as_deref().unwrap(),
				"old-tools",
				"Read it",
				AgentInputExtras { attachments: &[], task_references: &references }
			)
			.await,
		Err(AgentError::Rejected(_))
	));
	connection
		.execute("UPDATE agent_tool_versions SET version=3 WHERE work_id='agent'", [])
		.unwrap();
	references[0].thread_id = decodex_protocol::WireText::new("stale-thread").unwrap();
	assert!(matches!(
		agent
			.steer_work_with_references(
				"agent",
				work.active_turn_id.as_deref().unwrap(),
				"stale-ref",
				"Read it",
				AgentInputExtras { attachments: &[], task_references: &references }
			)
			.await,
		Err(AgentError::Rejected(_))
	));
	assert!(sent.try_recv().is_err());
	assert!(
		agent
			.store
			.read_agent_work_events("agent".into(), 100)
			.await
			.unwrap()
			.iter()
			.all(|e| e.event_kind != "steer_pending")
	);
}
