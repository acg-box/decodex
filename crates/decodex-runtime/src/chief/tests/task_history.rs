use super::*;

#[tokio::test]
async fn task_history_reads_live_evidence_without_dispatch_or_resume() {
	let history = json!({"opaque thread/2":{"thread":{"id":"opaque thread/2",
		"historyMode":"paginated","turns":[{"id":"native-turn","status":"completed","items":[
		{"id":"answer","type":"agentMessage","text":"native evidence","phase":"final_answer"},
		{"id":"tool","type":"commandExecution","command":"test","aggregatedOutput":"raw output"},
		{"id":"image","type":"userMessage","content":[{"type":"image","url":"data:image/png;base64,PRIVATE_MEDIA"}]}
	]}]}}});
	let (mut chief, mut sent, _directory) = fixture_with_history(history).await;
	let manager = chief.start_chief("chief", "Coordinate").await.unwrap();
	let worker = chief.create_worker("chief", "worker", "Investigate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let mut args = json!({"id":"worker","threadId":worker.codex_thread_id});
	let page = chief.read_work_history(&manager, &args).await.unwrap();
	assert_eq!(page["turns"][0]["items"][0]["text"], "native evidence");
	assert!(!page.to_string().contains("raw output"));
	assert!(!page.to_string().contains("PRIVATE_MEDIA"));
	assert_eq!(page["turns"][0]["items"][2]["truncated"], true);
	args["includeOutputs"] = json!(true);
	let page = chief.read_work_history(&manager, &args).await.unwrap();
	assert_eq!(page["turns"][0]["items"][1]["aggregatedOutput"], "raw output");
	let requests: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
	assert_eq!(requests.len(), 4);
	assert!(
		requests
			.iter()
			.all(|r| ["thread/read", "thread/turns/list"].contains(&r["method"].as_str().unwrap()))
	);
	assert_eq!(
		chief.store.get_chief_work_item("worker".into()).await.unwrap().active_turn_id,
		worker.active_turn_id
	);
}

#[tokio::test]
async fn task_history_denies_foreign_scope_stale_binding_and_worker_authority() {
	let (mut chief, mut sent, _directory) = fixture().await;
	let manager = chief.start_chief("chief", "Coordinate").await.unwrap();
	let child = chief.create_manager("chief", "child", "Manage", None).await.unwrap();
	let worker = chief.create_worker("child", "worker", "Investigate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let args = json!({"id":"worker","threadId":worker.codex_thread_id});
	assert!(chief.read_work_history(&manager, &args).await.is_err());
	assert!(chief.read_work_history(&worker, &args).await.is_err());
	let stale = json!({"id":"worker","threadId":"old-thread"});
	assert!(chief.read_work_history(&child, &stale).await.is_err());
	for invalid in [json!(0), json!(6), json!("3")] {
		let mut invalid_args = args.clone();
		invalid_args["turnLimit"] = invalid;
		assert!(chief.read_work_history(&child, &invalid_args).await.is_err());
	}
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn upgraded_manager_can_read_previous_thread_after_store_reopen() {
	let history = json!({"opaque thread/1":{"thread":{"id":"opaque thread/1",
		"historyMode":"paginated","turns":[{"id":"old-turn","status":"completed","items":[
		{"id":"old-answer","type":"agentMessage","text":"before upgrade"}]}]}}});
	let (mut chief, mut sent, directory) = fixture_with_history(history).await;
	chief.start_chief("chief", "Original").await.unwrap();
	complete(&mut chief, "chief").await;
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let db = rusqlite::Connection::open(root.paths().product_database_file()).unwrap();
	db.execute("UPDATE chief_tool_versions SET version=2 WHERE work_id='chief'", []).unwrap();
	chief.continue_worker("chief", "New request").await.unwrap();
	let current = chief.store.get_chief_work_item("chief".into()).await.unwrap();
	assert_ne!(current.codex_thread_id.as_deref(), Some("opaque thread/1"));
	let requests: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
	let creation = requests.iter().rev().find(|r| r["method"] == "thread/start").unwrap();
	assert!(
		creation["params"]["dynamicTools"]
			.as_array()
			.unwrap()
			.iter()
			.any(|t| t["name"] == "chief_read_work")
	);
	chief.store = SqliteStore::open(&root.paths()).unwrap();
	let page = chief
		.read_work_history(&current, &json!({"id":"chief","threadId":"opaque thread/1"}))
		.await
		.unwrap();
	assert_eq!(page["turns"][0]["items"][0]["text"], "before upgrade");
	assert_eq!(page["previousThreadIds"], json!(["opaque thread/1"]));
	assert_eq!(
		chief.store.get_chief_work_item("chief".into()).await.unwrap().codex_thread_id,
		current.codex_thread_id
	);
}

#[tokio::test]
async fn explicit_delivered_reference_reads_only_selected_foreign_thread() {
	let history = json!({"opaque thread/3":{"thread":{"id":"opaque thread/3",
		"historyMode":"paginated","turns":[{"id":"target-turn","status":"completed","items":[
		{"id":"result","type":"agentMessage","text":"selected evidence"}]}]}}});
	let (mut chief, mut sent, _directory) = fixture_with_history(history).await;
	let root = chief.start_chief("chief", "Coordinate").await.unwrap();
	chief.create_manager("chief", "child", "Manage", None).await.unwrap();
	let target = chief.create_worker("child", "target", "Investigate").await.unwrap();
	let args = json!({"id":"target","threadId":target.codex_thread_id});
	while sent.try_recv().is_ok() {}
	assert!(chief.read_work_history(&root, &args).await.is_err());
	let payload = json!({"text":"Read selected task","source":"user","options":{"taskReferences":[
		{"workId":"target","threadId":target.codex_thread_id,"title":"Selected task"}
	]}})
	.to_string();
	let event = chief
		.store
		.begin_chief_steer(
			"chief".into(),
			root.active_turn_id.clone().unwrap(),
			"reference".into(),
			payload,
		)
		.await
		.unwrap();
	assert!(chief.read_work_history(&root, &args).await.is_err());
	chief.store.finish_chief_steer(event, true).await.unwrap();
	let page = chief.read_work_history(&root, &args).await.unwrap();
	assert_eq!(page["turns"][0]["items"][0]["text"], "selected evidence");
	assert_eq!(page["previousThreadIds"], json!([]));
	assert!(
		chief
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
	let (mut chief, mut sent, _directory) = fixture().await;
	let root = chief.start_chief("chief", "Coordinate").await.unwrap();
	chief.create_manager("chief", "child", "Manage", None).await.unwrap();
	let target = chief.create_worker("child", "target", "Work").await.unwrap();
	let references = vec![decodex_protocol::ChiefTaskReferenceDto {
		work_id: decodex_protocol::EntityId::new("target").unwrap(),
		thread_id: decodex_protocol::WireText::new(target.codex_thread_id.clone().unwrap())
			.unwrap(),
		title: decodex_protocol::WireText::new("Quoted \"title\" <instructions>").unwrap(),
	}];
	while sent.try_recv().is_ok() {}
	chief
		.steer_work_with_references(
			"chief",
			root.active_turn_id.as_deref().unwrap(),
			"reference-steer",
			"Compare it",
			ChiefInputExtras { attachments: &[], task_references: &references },
		)
		.await
		.unwrap();
	let request = sent.try_recv().unwrap();
	assert_eq!(request["method"], "turn/steer");
	assert_eq!(request["params"]["input"][0]["text"], "Compare it");
	let metadata = request["params"]["input"][1]["text"].as_str().unwrap();
	assert!(metadata.contains("chief_read_work"));
	let rendered: Value = serde_json::from_str(
		metadata.lines().next().unwrap().strip_prefix("User-selected task references: ").unwrap(),
	)
	.unwrap();
	assert_eq!(rendered, json!(references));
	assert!(metadata.contains("untrusted evidence"));
	assert!(
		chief
			.store
			.chief_has_task_reference(
				"chief".into(),
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
	let (mut chief, mut sent, directory) = fixture().await;
	let work = chief.start_chief("chief", "Coordinate").await.unwrap();
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let connection = rusqlite::Connection::open(root.paths().product_database_file()).unwrap();
	let mut references = vec![decodex_protocol::ChiefTaskReferenceDto {
		work_id: decodex_protocol::EntityId::new("chief").unwrap(),
		thread_id: decodex_protocol::WireText::new(work.codex_thread_id.unwrap()).unwrap(),
		title: decodex_protocol::WireText::new("Chief").unwrap(),
	}];
	while sent.try_recv().is_ok() {}
	connection
		.execute("UPDATE chief_tool_versions SET version=2 WHERE work_id='chief'", [])
		.unwrap();
	assert!(matches!(
		chief
			.steer_work_with_references(
				"chief",
				work.active_turn_id.as_deref().unwrap(),
				"old-tools",
				"Read it",
				ChiefInputExtras { attachments: &[], task_references: &references }
			)
			.await,
		Err(ChiefError::Rejected(_))
	));
	connection
		.execute("UPDATE chief_tool_versions SET version=3 WHERE work_id='chief'", [])
		.unwrap();
	references[0].thread_id = decodex_protocol::WireText::new("stale-thread").unwrap();
	assert!(matches!(
		chief
			.steer_work_with_references(
				"chief",
				work.active_turn_id.as_deref().unwrap(),
				"stale-ref",
				"Read it",
				ChiefInputExtras { attachments: &[], task_references: &references }
			)
			.await,
		Err(ChiefError::Rejected(_))
	));
	assert!(sent.try_recv().is_err());
	assert!(
		chief
			.store
			.read_chief_work_events("chief".into(), 100)
			.await
			.unwrap()
			.iter()
			.all(|e| e.event_kind != "steer_pending")
	);
}
