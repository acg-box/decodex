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
