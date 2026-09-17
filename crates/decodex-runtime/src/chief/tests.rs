use super::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[path = "tests/capacity.rs"] mod capacity;

#[tokio::test]
async fn asynchronous_questions_and_usage_are_observed_without_completing_or_waking_work() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let message = json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","item":{
		"id":"question","type":"agentMessage","delivery":"async","text":"Which format?\n- PDF\n- Markdown",
		"questions":[{"title":"Which format?","options":["PDF","Markdown"]}]}});
	for _ in 0..2 {
		coordinator
			.handle_event(ServerEvent::Notification {
				method: "item/completed".into(),
				params: message.clone(),
			})
			.await
			.unwrap();
	}
	let counts = json!({"totalTokens":1200,"inputTokens":1000,"cachedInputTokens":500,"outputTokens":200,"reasoningOutputTokens":100});
	coordinator.handle_event(ServerEvent::Notification { method:"thread/tokenUsage/updated".into(), params:json!({
		"threadId":"opaque thread/1","turnId":"opaque turn/1","tokenUsage":{"total":counts,"last":counts,"modelContextWindow":128000}
	}) }).await.unwrap();
	coordinator.handle_event(ServerEvent::Notification { method:"item/completed".into(), params:json!({
		"threadId":"opaque thread/1","turnId":"opaque turn/1","item":{"type":"contextCompaction","id":"compact"}
	}) }).await.unwrap();
	let work = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	assert_eq!(work.dispatch_state, decodex_database::ChiefDispatchState::Running);
	let history = coordinator.store.read_chief_work_events("chief".into(), 10).await.unwrap();
	assert_eq!(history.len(), 2);
	assert_eq!(history[0].event_kind, "assistant_message");
	assert!(coordinator.store.list_pending_chief_events(10).await.unwrap().is_empty());
	coordinator.wake_pending().await.unwrap();
	assert!(sent.try_recv().is_err());
	coordinator.recover_persisted().await.unwrap();
	let usage = coordinator
		.store
		.read_chief_turn_usage("chief".into(), "opaque turn/1".into())
		.await
		.unwrap()
		.unwrap();
	assert_eq!(
		serde_json::from_str::<Value>(&usage.payload).unwrap()["tokenUsage"]["last"]["inputTokens"],
		1000
	);
}

#[path = "tests/inbox_carryover.rs"] mod inbox_carryover;

#[path = "tests/result_integrity.rs"] mod result_integrity;

async fn fixture()
-> (ChiefCoordinator, tokio::sync::mpsc::UnboundedReceiver<Value>, tempfile::TempDir) {
	fixture_with_history(json!({})).await
}

async fn fixture_with_history(
	history: Value,
) -> (ChiefCoordinator, tokio::sync::mpsc::UnboundedReceiver<Value>, tempfile::TempDir) {
	let directory = tempfile::tempdir().unwrap();
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	root.paths().ensure_layout().unwrap();
	let store = SqliteStore::open(&root.paths()).unwrap();
	let (client_io, server_io) = tokio::io::duplex(65536);
	let (reader, writer) = tokio::io::split(client_io);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	let (sent, received) = tokio::sync::mpsc::unbounded_channel();
	tokio::spawn(async move {
		let (reader, mut writer) = tokio::io::split(server_io);
		let mut lines = BufReader::new(reader).lines();
		let mut threads = 0;
		let mut turns = 0;
		while let Some(line) = lines.next_line().await.unwrap() {
			let request: Value = serde_json::from_str(&line).unwrap();
			sent.send(request.clone()).unwrap();
			if request.get("method").is_none() {
				continue;
			}
			let result = match request["method"].as_str() {
				Some("thread/read") => {
					let id = request["params"]["threadId"].as_str().unwrap();
					let mut result = history.get(id).cloned().unwrap_or_else(
						|| json!({"thread":{"id":id,"turns":[],"status":{"type":"idle"}}}),
					);
					if result["thread"]["historyMode"] == "paginated" {
						assert_ne!(request["params"]["includeTurns"], true);
						result["thread"]["turns"] = json!([]);
					}
					result
				},
				Some("thread/turns/list") => {
					let id = request["params"]["threadId"].as_str().unwrap();
					let mut turns = history[id]["thread"]["turns"].clone();
					for turn in turns.as_array_mut().unwrap() {
						turn["items"] = json!([]);
						turn["itemsView"] = json!("notLoaded");
					}
					json!({"data":turns,"nextCursor":null})
				},
				Some("thread/items/list") => {
					let id = request["params"]["threadId"].as_str().unwrap();
					let turn_id = &request["params"]["turnId"];
					let turn = history[id]["thread"]["turns"]
						.as_array()
						.unwrap()
						.iter()
						.find(|turn| turn["id"] == *turn_id)
						.unwrap();
					let entries: Vec<_> = turn["items"]
						.as_array()
						.unwrap()
						.iter()
						.map(|item| json!({"turnId":turn_id,"item":item}))
						.collect();
					json!({"data":entries,"nextCursor":null})
				},
				Some("thread/resume") => {
					json!({"thread":{"id":request["params"]["threadId"]},"model":"selected-model","reasoningEffort":request["params"]["config"]["model_reasoning_effort"]})
				},
				Some("thread/start") => {
					threads += 1;
					json!({"thread":{"id":format!("opaque thread/{threads}")},"model":"selected-model","reasoningEffort":request["params"]["config"]["model_reasoning_effort"]})
				},
				Some("turn/start") => {
					turns += 1;
					json!({"turn":{"id":format!("opaque turn/{turns}")}})
				},
				_ => json!({}),
			};
			let mut frame = json!({"id":request["id"],"result":result}).to_string();
			frame.push('\n');
			writer.write_all(frame.as_bytes()).await.unwrap();
		}
	});
	(
		ChiefCoordinator::new(
			store,
			client,
			ChiefConfig::new(
				"selected-model".into(),
				"high".into(),
				directory.path().display().to_string(),
			),
		)
		.unwrap(),
		received,
		directory,
	)
}

#[tokio::test]
async fn unloaded_thread_resumes_exact_identity_without_new_thread() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	complete(&mut coordinator, "chief").await;
	let original = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "thread/resume");
	}
	coordinator
		.handle_event(ServerEvent::Notification {
			method: "thread/closed".into(),
			params: json!({"threadId":original.codex_thread_id}),
		})
		.await
		.unwrap();
	coordinator.continue_worker("chief", "Continue the original Chief").await.unwrap();
	let mut resumes = Vec::new();
	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "thread/start");
		if request["method"] == "thread/resume" {
			resumes.push(request);
		}
	}
	assert_eq!(resumes.len(), 1);
	assert_eq!(resumes[0]["params"]["threadId"], json!(original.codex_thread_id));
}

#[tokio::test]
async fn automation_delivery_is_deduplicated_across_later_chief_turns() {
	let (mut coordinator, _sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	complete(&mut coordinator, "chief").await;
	coordinator
		.ingest_automation_result("feed:event:1", "chief", json!({"result":"review requested"}))
		.await
		.unwrap();
	let first = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	assert!(first.active_turn_id.is_some());
	coordinator
		.ingest_automation_result("feed:event:1", "chief", json!({"result":"review requested"}))
		.await
		.unwrap();
	assert_eq!(
		coordinator.store.get_chief_work_item("chief".into()).await.unwrap().active_turn_id,
		first.active_turn_id
	);
	complete(&mut coordinator, "chief").await;
	coordinator
		.ingest_automation_result("feed:event:1", "chief", json!({"result":"review requested"}))
		.await
		.unwrap();
	assert!(
		coordinator
			.store
			.get_chief_work_item("chief".into())
			.await
			.unwrap()
			.active_turn_id
			.is_none()
	);
	assert_eq!(
		coordinator
			.store
			.list_pending_chief_events(100)
			.await
			.unwrap()
			.iter()
			.filter(|event| event.event_kind == "automation_result")
			.count(),
		1
	);
}

#[tokio::test]
async fn dependencies_block_turns_until_explicit_resolution() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	coordinator.create_worker("chief", "first", "Inspect").await.unwrap();
	let blocked = coordinator
		.create_worker_with_dependencies("chief", "second", "Use inspection", vec!["first".into()])
		.await
		.unwrap();
	assert!(blocked.active_turn_id.is_none());
	assert!(blocked.codex_thread_id.is_none());
	while sent.try_recv().is_ok() {}
	assert!(
		matches!(coordinator.continue_worker("second","Proceed").await,Err(ChiefError::DependenciesPending(ids)) if ids==vec!["first"])
	);
	assert!(sent.try_recv().is_err());
	complete(&mut coordinator, "first").await;
	assert!(matches!(
		coordinator.continue_worker("second", "Proceed").await,
		Err(ChiefError::DependenciesPending(_))
	));
	coordinator
		.store
		.set_chief_work_status("first".into(), ChiefWorkStatus::Resolved, None)
		.await
		.unwrap();
	let mut coordinator = ChiefCoordinator::new(
		coordinator.store.clone(),
		coordinator.client.clone(),
		coordinator.config.clone(),
	)
	.unwrap();
	coordinator.continue_worker("second", "Proceed with accepted inspection").await.unwrap();
	assert!(
		coordinator
			.store
			.get_chief_work_item("second".into())
			.await
			.unwrap()
			.active_turn_id
			.is_some()
	);
}

#[tokio::test]
async fn wait_requires_future_due_and_due_checks_wake_once_per_timestamp() {
	let (mut coordinator, _sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	complete(&mut coordinator, "chief").await;
	coordinator
		.ingest_automation_result("source:wait", "chief", json!({"result":"not ready"}))
		.await
		.unwrap();
	let chief = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let event = coordinator
		.store
		.list_pending_chief_events(100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.event_kind == "automation_result")
		.unwrap();
	let mut args =
		json!({"id":"chief","eventIds":[event.id],"status":"wait","summary":"Check source again"});
	assert!(
		coordinator
			.tool(&chief, &json!({"tool":"chief_disposition","arguments":args}))
			.await
			.is_err()
	);
	let due = now_micros().unwrap() + 60_000_000;
	args["nextCheckAtMicros"] = json!(due);
	coordinator.tool(&chief, &json!({"tool":"chief_disposition","arguments":args})).await.unwrap();
	complete(&mut coordinator, "chief").await;
	coordinator.check_due_followups(due - 1).await.unwrap();
	assert!(
		coordinator
			.store
			.get_chief_work_item("chief".into())
			.await
			.unwrap()
			.active_turn_id
			.is_none()
	);
	coordinator.check_due_followups(due).await.unwrap();
	assert!(
		coordinator
			.store
			.get_chief_work_item("chief".into())
			.await
			.unwrap()
			.active_turn_id
			.is_some()
	);
	complete(&mut coordinator, "chief").await;
	coordinator.check_due_followups(due + 1).await.unwrap();
	assert!(
		coordinator
			.store
			.get_chief_work_item("chief".into())
			.await
			.unwrap()
			.active_turn_id
			.is_none()
	);
	assert_eq!(
		coordinator
			.store
			.list_pending_chief_events(100)
			.await
			.unwrap()
			.iter()
			.filter(|event| event.event_kind == "followup_due")
			.count(),
		1
	);
}

#[tokio::test]
async fn failed_thread_start_remains_unknown_without_retry() {
	let (mut coordinator, _sent, _directory) = fixture().await;
	coordinator.client.shutdown().await.unwrap();
	assert!(coordinator.start_chief("chief", "Coordinate").await.is_err());
	let work = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	assert_eq!(work.dispatch_state, decodex_database::ChiefDispatchState::Unknown);
	assert!(work.codex_thread_id.is_none());
	coordinator.recover_persisted().await.unwrap();
	assert_eq!(
		coordinator.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		decodex_database::ChiefDispatchState::Unknown
	);
	assert!(matches!(
		coordinator.continue_worker("chief", "Retry").await,
		Err(ChiefError::UnknownDispatch)
	));
}

#[tokio::test]
async fn recovery_records_only_exact_terminal_evidence_without_dispatching() {
	let history = json!({
		"opaque thread/1":{"thread":{"id":"opaque thread/1","status":{"type":"idle"},"turns":[{"id":"opaque turn/1","status":"completed","items":[{"type":"agentMessage","text":"Chief waiting"}]}]}},
		"opaque thread/2":{"thread":{"id":"opaque thread/2","status":{"type":"idle"},"turns":[{"id":"opaque turn/2","status":"failed","items":[{"type":"agentMessage","text":"Worker exact evidence"}]}]}}
	});
	let (mut coordinator, mut sent, _directory) = fixture_with_history(history).await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	coordinator.create_goal("chief", "goal", "Goal").await.unwrap();
	coordinator.create_worker("goal", "worker", "Inspect").await.unwrap();
	while sent.try_recv().is_ok() {}
	let mut recovered = ChiefCoordinator::new(
		coordinator.store.clone(),
		coordinator.client.clone(),
		coordinator.config.clone(),
	)
	.unwrap();
	recovered.recover_persisted().await.unwrap();
	for id in ["chief", "worker"] {
		assert_eq!(
			recovered.store.get_chief_work_item(id.into()).await.unwrap().dispatch_state,
			decodex_database::ChiefDispatchState::Idle
		);
	}
	let events = recovered.store.list_pending_chief_events(100).await.unwrap();
	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_kind, "worker_turn_completed");
	assert!(events[0].payload.contains("Worker exact evidence"));
	assert!(events[0].payload.contains("failed"));
	while let Ok(request) = sent.try_recv() {
		assert!(["thread/resume", "thread/read"].contains(&request["method"].as_str().unwrap()));
	}
	recovered.recover_persisted().await.unwrap();
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn recovery_missing_exact_turn_preserves_unknown_without_replay() {
	let history = json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"different-turn","status":"completed","items":[]}]}}});
	let (mut coordinator, mut sent, _directory) = fixture_with_history(history).await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	coordinator.recover_persisted().await.unwrap();
	let work = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	assert_eq!(work.dispatch_state, decodex_database::ChiefDispatchState::Unknown);
	assert_eq!(work.active_turn_id.as_deref(), Some("opaque turn/1"));
	assert!(coordinator.store.list_pending_chief_events(100).await.unwrap().is_empty());
	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start");
		assert_ne!(request["method"], "thread/start");
	}
	assert!(matches!(
		coordinator.continue_worker("chief", "Do not replay").await,
		Err(ChiefError::UnknownDispatch)
	));
}

#[tokio::test]
async fn recovery_preserves_positive_active_turn_observation() {
	let history = json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","status":{"type":"active","activeFlags":[]},"turns":[{"id":"opaque turn/1","status":"inProgress","items":[]}]}}});
	let (mut coordinator, mut sent, _directory) = fixture_with_history(history).await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	coordinator.recover_persisted().await.unwrap();
	assert_eq!(
		coordinator.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		decodex_database::ChiefDispatchState::Running
	);
	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start");
	}
}

#[tokio::test]
async fn initial_user_input_starts_once_and_receipt_ack_leaves_newer_input_pending() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	ChiefCoordinator::reserve_root(&coordinator.store, "chief", "Personal Chief").await.unwrap();
	coordinator
		.enqueue_user_message("chief", "command-1", "Handle my actual request")
		.await
		.unwrap();
	coordinator.wake_pending().await.unwrap();
	coordinator.wake_pending().await.unwrap();
	let work = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	coordinator.enqueue_user_message("chief", "command-2", "A later request").await.unwrap();
	coordinator.record_terminal(json!({"threadId":work.codex_thread_id,"turn":{"id":work.active_turn_id,"status":"completed","items":[]}}),Ok(json!({}))).await.unwrap();
	let pending = coordinator.store.list_pending_chief_events(100).await.unwrap();
	assert_eq!(pending.len(), 1);
	assert!(pending[0].payload.contains("A later request"));
	assert!(pending[0].delivered_turn_id.is_none());
	let starts: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok())
		.filter(|request| request["method"] == "turn/start")
		.collect();
	assert_eq!(starts.len(), 1);
	assert!(
		starts[0]["params"]["input"][0]["text"]
			.as_str()
			.unwrap()
			.contains("direct request from the user")
	);
	coordinator.wake_pending().await.unwrap();
	complete(&mut coordinator, "chief").await;
	coordinator
		.enqueue_user_message("chief", "command-1", "Handle my actual request")
		.await
		.unwrap();
	coordinator.wake_pending().await.unwrap();
	assert!(
		coordinator
			.store
			.get_chief_work_item("chief".into())
			.await
			.unwrap()
			.active_turn_id
			.is_none()
	);
	assert!(coordinator.store.list_pending_chief_events(100).await.unwrap().is_empty());
}

#[tokio::test]
async fn permission_response_uses_live_event_identity_even_when_rpc_id_is_reused() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	let root = coordinator.start_chief("chief", "Coordinate").await.unwrap();
	let params = json!({"threadId":root.codex_thread_id,"turnId":root.active_turn_id});
	coordinator
		.handle_event(ServerEvent::Request {
			id: RequestId::Number(7),
			method: "item/commandExecution/requestApproval".into(),
			params: params.clone(),
		})
		.await
		.unwrap();
	let old_id = coordinator.store.list_pending_chief_events(100).await.unwrap()[0].id;
	let mut reconnected = ChiefCoordinator::new(
		coordinator.store.clone(),
		coordinator.client.clone(),
		coordinator.config.clone(),
	)
	.unwrap();
	reconnected
		.handle_event(ServerEvent::Request {
			id: RequestId::Number(7),
			method: "item/commandExecution/requestApproval".into(),
			params,
		})
		.await
		.unwrap();
	let new_id = reconnected
		.store
		.list_pending_chief_events(100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.id != old_id)
		.unwrap()
		.id;
	while sent.try_recv().is_ok() {}
	assert!(
		reconnected.respond_pending_event(old_id, json!({"decision":"decline"})).await.is_err()
	);
	assert!(sent.try_recv().is_err());
	reconnected.respond_pending_event(new_id, json!({"decision":"decline"})).await.unwrap();
	let response = sent.recv().await.unwrap();
	assert_eq!(response, json!({"id":7,"result":{"decision":"decline"}}));
	assert!(
		reconnected.respond_pending_event(new_id, json!({"decision":"decline"})).await.is_err()
	);
	assert_eq!(
		reconnected.store.get_chief_work_item("chief".into()).await.unwrap().status,
		ChiefWorkStatus::Open
	);
	let pending = reconnected.store.list_pending_chief_events(100).await.unwrap();
	assert_eq!(pending.len(), 1);
	assert_eq!(pending[0].id, old_id);
}

#[tokio::test]
async fn resolving_prerequisite_releases_authorized_unbound_worker_once() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	coordinator.create_worker("chief", "first", "Inspect").await.unwrap();
	coordinator
		.create_worker_with_dependencies("chief", "second", "Use the result", vec!["first".into()])
		.await
		.unwrap();
	complete(&mut coordinator, "chief").await;
	complete(&mut coordinator, "first").await;
	let chief = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let event = coordinator
		.store
		.list_chief_events_for_turn(chief.active_turn_id.clone().unwrap(), 100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.work_item_id == "first")
		.unwrap();
	while sent.try_recv().is_ok() {}
	let result=coordinator.tool(&chief,&json!({"tool":"chief_disposition","arguments":{"id":"first","status":"resolved","summary":"Evidence accepted","eventIds":[event.id]}})).await.unwrap();
	assert_eq!(result["releasedWorkIds"], json!(["second"]));
	let second = coordinator.store.get_chief_work_item("second".into()).await.unwrap();
	assert!(second.codex_thread_id.is_some());
	assert!(second.active_turn_id.is_some());
	assert!(coordinator.release_ready_workers("chief").await.unwrap().is_empty());
	let requests: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
	assert_eq!(requests.iter().filter(|request| request["method"] == "thread/start").count(), 1);
	assert_eq!(requests.iter().filter(|request| request["method"] == "turn/start").count(), 1);
}

async fn complete(coordinator: &mut ChiefCoordinator, id: &str) {
	let work = coordinator.store.get_chief_work_item(id.into()).await.unwrap();
	coordinator.handle_event(ServerEvent::Notification {
        method:"turn/completed".into(),params:json!({"threadId":work.codex_thread_id,"turn":{"id":work.active_turn_id,"status":"completed","items":[{"type":"agentMessage","text":"result"}]}})
    }).await.unwrap();
}

#[tokio::test]
async fn goal_completion_requires_explicit_judgment_and_related_evidence() {
	let (mut coordinator, _sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	coordinator.create_goal("chief", "goal", "Outcome").await.unwrap();
	coordinator.create_goal("chief", "other", "Different outcome").await.unwrap();
	coordinator.create_worker("goal", "worker", "Inspect").await.unwrap();
	complete(&mut coordinator, "chief").await;
	complete(&mut coordinator, "worker").await;
	let chief = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let event = coordinator
		.store
		.list_chief_events_for_turn(chief.active_turn_id.clone().unwrap(), 100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.work_item_id == "worker")
		.unwrap();
	coordinator.tool(&chief,&json!({"tool":"chief_disposition","arguments":{"id":"worker","status":"resolved","eventIds":[event.id],"summary":"Result accepted"}})).await.unwrap();
	assert_eq!(
		coordinator.store.get_chief_work_item("goal".into()).await.unwrap().status,
		ChiefWorkStatus::Open
	);
	let command = |id| json!({"tool":"chief_resolve_goal","arguments":{"id":id,"evidenceEventId":event.id,"summary":"The accepted evidence satisfies this goal"}});
	assert!(coordinator.tool(&chief, &command("other")).await.is_err());
	coordinator.tool(&chief, &command("goal")).await.unwrap();
	assert_eq!(
		coordinator.store.get_chief_work_item("goal".into()).await.unwrap().status,
		ChiefWorkStatus::Resolved
	);
	assert!(
		coordinator
			.store
			.read_chief_work_events("goal".into(), 10)
			.await
			.unwrap()
			.iter()
			.any(|entry| entry.event_kind == "goal_resolved")
	);
	assert!(coordinator.tool(&chief, &command("goal")).await.is_err());
}

#[tokio::test]
async fn explicit_user_reply_resolves_worker_decision_without_rewriting_old_evidence() {
	let (mut coordinator, _sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	coordinator.create_worker("chief", "worker", "Inspect").await.unwrap();
	complete(&mut coordinator, "chief").await;
	complete(&mut coordinator, "worker").await;
	let chief = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let event = coordinator
		.store
		.list_chief_events_for_turn(chief.active_turn_id.clone().unwrap(), 100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.work_item_id == "worker")
		.unwrap();
	coordinator.tool(&chief,&json!({"tool":"chief_disposition","arguments":{"id":"worker","status":"user_decision","summary":"Choose A or B","eventIds":[event.id]}})).await.unwrap();
	complete(&mut coordinator, "chief").await;
	coordinator
		.enqueue_user_message("chief", "answer-once", "Choose A and accept this result")
		.await
		.unwrap();
	coordinator.wake_pending().await.unwrap();
	let chief = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let user = coordinator
		.store
		.list_chief_events_for_turn(chief.active_turn_id.clone().unwrap(), 100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.event_kind == "user_message")
		.unwrap();
	let command = |user_id| json!({"tool":"chief_resolve_decision","arguments":{"id":"worker","userEventId":user_id,"summary":"User selected A; result accepted"}});
	assert!(coordinator.tool(&chief, &command(event.id)).await.is_err());
	coordinator.tool(&chief, &command(user.id)).await.unwrap();
	assert_eq!(
		coordinator.store.get_chief_work_item("worker".into()).await.unwrap().status,
		ChiefWorkStatus::Resolved
	);
	assert_eq!(
		coordinator.store.get_chief_inbox_event(event.id).await.unwrap().disposition,
		Some(ChiefDisposition::UserDecision)
	);
	let evidence = coordinator.store.read_chief_work_events("worker".into(), 100).await.unwrap();
	assert!(evidence.iter().any(|item| item.event_kind == "user_decision_resolved"
		&& item.payload.contains(&format!("\"userEventId\":{}", user.id))));
	assert!(coordinator.tool(&chief, &command(user.id)).await.is_err());
	assert!(coordinator.store.get_chief_inbox_event(user.id).await.unwrap().disposition.is_none());
}

#[tokio::test]
async fn independent_workers_queue_then_wake_and_continue_same_identity() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate two workers").await.unwrap();
	coordinator.create_goal("chief", "goal-a", "Goal A").await.unwrap();
	coordinator.create_goal("chief", "goal-b", "Goal B").await.unwrap();
	let first = coordinator.create_worker("goal-a", "first", "Inspect A").await.unwrap();
	let second = coordinator.create_worker("goal-b", "second", "Inspect B").await.unwrap();
	assert_ne!(first.codex_thread_id, second.codex_thread_id);
	complete(&mut coordinator, "first").await;
	assert_eq!(
		coordinator
			.store
			.get_chief_work_item("chief".into())
			.await
			.unwrap()
			.active_turn_id
			.as_deref(),
		Some("opaque turn/1")
	);
	complete(&mut coordinator, "chief").await;
	let resumed = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	assert_eq!(resumed.active_turn_id.as_deref(), Some("opaque turn/4"));
	coordinator.continue_worker("first", "Repair missing evidence").await.unwrap();
	assert_eq!(
		coordinator.store.get_chief_work_item("first".into()).await.unwrap().codex_thread_id,
		first.codex_thread_id
	);
	complete(&mut coordinator, "chief").await;
	coordinator.wake_pending().await.unwrap();
	assert!(
		coordinator
			.store
			.get_chief_work_item("chief".into())
			.await
			.unwrap()
			.active_turn_id
			.is_none()
	);
	complete(&mut coordinator, "second").await;
	assert!(
		coordinator
			.store
			.get_chief_work_item("chief".into())
			.await
			.unwrap()
			.active_turn_id
			.is_some()
	);
	let mut starts = Vec::new();
	while let Ok(request) = sent.try_recv() {
		if request["method"] == "turn/start" {
			starts.push(request);
		}
	}
	assert_eq!(starts.len(), 6);
	assert_eq!(starts[4]["params"]["threadId"], json!(first.codex_thread_id));
	assert_eq!(starts[1]["params"]["effort"], "medium");
	assert_eq!(starts[3]["params"]["threadId"], json!(resumed.codex_thread_id));
}

#[tokio::test]
async fn disposition_cannot_consume_undelivered_events_and_requests_stay_pending() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	coordinator.create_worker("chief", "first", "Inspect").await.unwrap();
	complete(&mut coordinator, "chief").await;
	complete(&mut coordinator, "first").await;
	let chief = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let delivered = coordinator
		.store
		.list_pending_chief_events(100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.work_item_id == "first")
		.unwrap();
	let newer = coordinator
		.store
		.enqueue_chief_event(EnqueueChiefEvent {
			source_event_id: "automation:new".into(),
			work_item_id: "first".into(),
			event_kind: "automation_result".into(),
			payload: "new facts".into(),
		})
		.await
		.unwrap();
	let params = json!({"tool":"chief_disposition","arguments":{"id":"first","status":"resolved","summary":"Reviewed result","eventIds":[newer.id]}});
	assert!(coordinator.tool(&chief, &params).await.is_err());
	let params = json!({"tool":"chief_disposition","arguments":{"id":"first","status":"resolved","summary":"Reviewed result","eventIds":[delivered.id]}});
	coordinator.tool(&chief, &params).await.unwrap();
	assert!(
		coordinator
			.store
			.list_pending_chief_events(100)
			.await
			.unwrap()
			.iter()
			.any(|event| event.id == newer.id)
	);
	while sent.try_recv().is_ok() {}
	coordinator
		.handle_event(ServerEvent::Request {
			id: RequestId::String("approval opaque".into()),
			method: "item/commandExecution/requestApproval".into(),
			params: json!({"threadId":chief.codex_thread_id}),
		})
		.await
		.unwrap();
	assert!(sent.try_recv().is_err());
	assert!(
		coordinator
			.store
			.list_pending_chief_events(100)
			.await
			.unwrap()
			.iter()
			.any(|event| event.event_kind == "permission_pending")
	);
}

#[tokio::test]
async fn unknown_dispatch_and_unprocessed_events_survive_restart() {
	let (mut coordinator, _sent, directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	coordinator.create_worker("chief", "first", "Inspect").await.unwrap();
	complete(&mut coordinator, "first").await;
	let paths =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap()
			.paths();
	coordinator.store.begin_chief_dispatch("first".into()).await.unwrap();
	coordinator.store.mark_chief_dispatch_unknown("first".into()).await.unwrap();
	drop(coordinator);
	let reopened = SqliteStore::open(&paths).unwrap();
	assert!(reopened.begin_chief_dispatch("first".into()).await.is_err());
	assert_eq!(reopened.list_pending_chief_events(100).await.unwrap().len(), 1);
}
