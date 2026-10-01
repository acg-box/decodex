use std::iter;

use rusqlite::Connection;

use crate::agent::tests::{self, AgentError, AgentInputExtras, SqliteStore, Value};
use decodex_core::DecodexRoot;
use decodex_protocol::WireText;

#[test]
fn saved_task_references_are_rendered_on_queued_native_turn_input() {
	let mut params = serde_json::json!({"input":[{"type":"text","text":"Compare it"}]});
	let payload=serde_json::json!({"options":{"attachments":[],"taskReferences":[{"workId":"target","threadId":"native-thread","title":"Reference title"}]}}).to_string();

	tests::apply_message_options(&mut params, &payload).unwrap();

	assert_eq!(params["input"].as_array().unwrap().len(), 2);
	assert!(params["input"][1]["text"].as_str().unwrap().contains("native-thread"));
}

#[tokio::test]
async fn task_history_reads_live_evidence_without_dispatch_or_resume() {
	let history = serde_json::json!({"opaque thread/2":{"thread":{"id":"opaque thread/2",
		"historyMode":"paginated","turns":[{"id":"native-turn","status":"completed","items":[
		{"id":"answer","type":"agentMessage","text":"native evidence","phase":"final_answer"},
		{"id":"tool","type":"commandExecution","command":"test","aggregatedOutput":"raw output"},
		{"id":"image","type":"userMessage","content":[{"type":"image","url":"data:image/png;base64,PRIVATE_MEDIA"}]}
	]}]}}});
	let (mut agent, mut sent, _directory) = tests::fixture_with_history(history).await;
	let manager = agent.start_agent("agent", "Coordinate").await.unwrap();
	let worker = agent.create_worker("agent", "worker", "Investigate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let mut args = serde_json::json!({"id":"worker","threadId":worker.codex_thread_id});
	let page = agent.read_work_history(&manager, &args).await.unwrap();

	assert_eq!(page["turns"][0]["items"][0]["text"], "native evidence");
	assert!(!page.to_string().contains("raw output"));
	assert!(!page.to_string().contains("PRIVATE_MEDIA"));
	assert_eq!(page["turns"][0]["items"][2]["truncated"], true);

	args["includeOutputs"] = serde_json::json!(true);

	let page = agent.read_work_history(&manager, &args).await.unwrap();

	assert_eq!(page["turns"][0]["items"][1]["aggregatedOutput"], "raw output");

	let requests: Vec<_> = iter::from_fn(|| sent.try_recv().ok()).collect();

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
	let (mut agent, mut sent, _directory) = tests::fixture().await;
	let manager = agent.start_agent("agent", "Coordinate").await.unwrap();
	let child = agent.create_manager("agent", "child", "Manage", None).await.unwrap();
	let worker = agent.create_worker("child", "worker", "Investigate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let args = serde_json::json!({"id":"worker","threadId":worker.codex_thread_id});

	assert!(agent.read_work_history(&manager, &args).await.is_err());
	assert!(agent.read_work_history(&worker, &args).await.is_err());

	let stale = serde_json::json!({"id":"worker","threadId":"old-thread"});

	assert!(agent.read_work_history(&child, &stale).await.is_err());

	for invalid in [serde_json::json!(0), serde_json::json!(6), serde_json::json!("3")] {
		let mut invalid_args = args.clone();

		invalid_args["turnLimit"] = invalid;

		assert!(agent.read_work_history(&child, &invalid_args).await.is_err());
	}

	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn upgraded_manager_can_read_previous_thread_after_store_reopen() {
	let history = serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1",
		"historyMode":"paginated","turns":[{"id":"old-turn","status":"completed","items":[
		{"id":"old-answer","type":"agentMessage","text":"before upgrade"}]}]}}});
	let (mut agent, _sent, directory) = tests::fixture_with_history(history).await;

	agent.start_agent("agent", "Original").await.unwrap();

	tests::complete(&mut agent, "agent").await;

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
	// Seed a migration completed by an older release; current code never upgrades threads.
	let database = Connection::open(root.paths().product_database_file()).unwrap();

	database
		.execute_batch(
			"UPDATE agent_work_items SET codex_thread_id='historic-new-thread' WHERE id='agent';
		 INSERT INTO agent_thread_revisions(work_id,old_thread_id,new_thread_id,created_at_micros)
		 VALUES('agent','opaque thread/1','historic-new-thread',1);",
		)
		.unwrap();

	drop(database);

	let current = agent.store.get_agent_work_item("agent".into()).await.unwrap();

	agent.store = SqliteStore::open(&root.paths()).unwrap();

	let page = agent
		.read_work_history(
			&current,
			&serde_json::json!({"id":"agent","threadId":"opaque thread/1"}),
		)
		.await
		.unwrap();

	assert_eq!(page["turns"][0]["items"][0]["text"], "before upgrade");
	assert_eq!(page["previousThreadIds"], serde_json::json!(["opaque thread/1"]));
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().codex_thread_id,
		current.codex_thread_id
	);
}

#[tokio::test]
async fn explicit_delivered_reference_reads_only_selected_foreign_thread() {
	let history = serde_json::json!({"opaque thread/3":{"thread":{"id":"opaque thread/3",
		"historyMode":"paginated","turns":[{"id":"target-turn","status":"completed","items":[
		{"id":"result","type":"agentMessage","text":"selected evidence"}]}]}}});
	let (mut agent, mut sent, _directory) = tests::fixture_with_history(history).await;
	let root = agent.start_agent("agent", "Coordinate").await.unwrap();

	agent.create_manager("agent", "child", "Manage", None).await.unwrap();

	let target = agent.create_worker("child", "target", "Investigate").await.unwrap();
	let args = serde_json::json!({"id":"target","threadId":target.codex_thread_id});

	while sent.try_recv().is_ok() {}

	assert!(agent.read_work_history(&root, &args).await.is_err());

	let payload = serde_json::json!({"text":"Read selected task","source":"user","options":{"taskReferences":[
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
	assert_eq!(
		agent
			.store
			.agent_task_reference_target("agent".into(), target.codex_thread_id.clone().unwrap())
			.await
			.unwrap(),
		None
	);

	agent.store.finish_agent_steer(event, true).await.unwrap();

	assert_eq!(
		agent
			.store
			.agent_task_reference_target("agent".into(), target.codex_thread_id.clone().unwrap())
			.await
			.unwrap()
			.as_deref(),
		Some("target")
	);

	let page = agent.read_work_history(&root, &args).await.unwrap();

	assert_eq!(page["turns"][0]["items"][0]["text"], "selected evidence");
	assert_eq!(page["previousThreadIds"], serde_json::json!([]));
	assert!(
		agent
			.read_work_history(&root, &serde_json::json!({"id":"target","threadId":"unselected"}))
			.await
			.is_err()
	);

	let requests: Vec<_> = iter::from_fn(|| sent.try_recv().ok()).collect();

	assert_eq!(requests.len(), 2);
	assert!(
		requests
			.iter()
			.all(|r| ["thread/read", "thread/turns/list"].contains(&r["method"].as_str().unwrap()))
	);
}

#[tokio::test]
async fn native_steer_carries_typed_reference_and_only_acknowledgment_grants_read() {
	let (mut agent, mut sent, _directory) = tests::fixture().await;
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

	assert_eq!(rendered, serde_json::json!(references));
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

#[tokio::test]
async fn stale_reference_and_old_running_tools_reject_before_native_steer() {
	let (mut agent, mut sent, directory) = tests::fixture().await;
	let work = agent.start_agent("agent", "Coordinate").await.unwrap();
	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
	let connection = Connection::open(root.paths().product_database_file()).unwrap();
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

	references[0].thread_id = WireText::new("stale-thread").unwrap();

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

#[tokio::test]
async fn native_search_filters_foreign_work_and_preserves_cursor_and_exact_sources() {
	let history = serde_json::json!({"_search":{"data":[
		{"thread":{"id":"opaque thread/1","name":"Root"},"snippet":"matched body"},
		{"thread":{"id":"foreign-thread","name":"PRIVATE_TITLE"},"snippet":"PRIVATE_BODY"}

	],"nextCursor":"more"},"_occurrences":{"data":[{"turnId":"turn-hit","itemId":"item-hit",
		"snippet":"matched body","snippetMatchRange":{"start":0,"end":7},"turnCursor":"exact-turn"}],"nextCursor":null}});
	let (mut agent, mut sent, _directory) = tests::fixture_with_history(history).await;
	let manager = agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let page = agent
		.read_work_history(&manager, &serde_json::json!({"searchTerm":"matched","archived":true}))
		.await
		.unwrap();

	assert_eq!(page["matches"].as_array().unwrap().len(), 1);
	assert_eq!(page["nextCursor"], "more");
	assert_eq!(page["matches"][0]["sourceUrl"], "codex://threads/opaque%20thread%2F1");
	assert!(!page.to_string().contains("PRIVATE"));

	let hits = agent
		.read_work_history(
			&manager,
			&serde_json::json!({"id":"agent","threadId":manager.codex_thread_id,"searchTerm":"matched"}),
		)
		.await
		.unwrap();

	assert_eq!(hits["occurrences"][0]["turnCursor"], "exact-turn");
	assert_eq!(hits["occurrences"][0]["itemId"], "item-hit");
	assert_eq!(hits["occurrences"][0]["rangeEncoding"], "utf16");

	let requests: Vec<_> = iter::from_fn(|| sent.try_recv().ok()).collect();

	assert_eq!(requests.len(), 2);
	assert_eq!(requests[0]["method"], "thread/search");
	assert_eq!(requests[0]["params"]["archived"], true);
	assert_eq!(requests[1]["method"], "thread/searchOccurrences");
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().active_turn_id,
		manager.active_turn_id
	);
}

#[tokio::test]
async fn native_search_keeps_empty_filtered_pages_and_rejects_repeated_cursors() {
	let (mut agent, _sent, _directory) =
		tests::fixture_with_history(serde_json::json!({"_search":{"data":[
		{"thread":{"id":"foreign-thread"},"snippet":"hidden"}],"nextCursor":"more"}}))
		.await;
	let manager = agent.start_agent("agent", "Coordinate").await.unwrap();
	let page =
		agent.read_work_history(&manager, &serde_json::json!({"searchTerm":"term"})).await.unwrap();

	assert_eq!(page["matches"], serde_json::json!([]));
	assert_eq!(page["nextCursor"], "more");
	assert!(
		agent
			.read_work_history(&manager, &serde_json::json!({"searchTerm":"term","cursor":"more"}))
			.await
			.is_err()
	);
}

#[tokio::test]
async fn native_occurrence_search_rejects_foreign_scope_before_rpc() {
	let (mut agent, mut sent, _directory) = tests::fixture().await;
	let manager = agent.start_agent("agent", "Coordinate").await.unwrap();
	let _child = agent.create_manager("agent", "child", "Manage", None).await.unwrap();
	let worker = agent.create_worker("child", "worker", "Investigate").await.unwrap();

	while sent.try_recv().is_ok() {}

	assert!(
		agent
			.read_work_history(
				&manager,
				&serde_json::json!({"id":"worker","threadId":worker.codex_thread_id,"searchTerm":"secret"})
			)
			.await
			.is_err()
	);
	assert!(
		agent
			.read_work_history(&worker, &serde_json::json!({"searchTerm":"secret"}))
			.await
			.is_err()
	);
	assert!(
		agent
			.read_work_history(
				&manager,
				&serde_json::json!({"threadId":"foreign","searchTerm":"secret"})
			)
			.await
			.is_err()
	);
	assert!(sent.try_recv().is_err());
}
