use super::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

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
		let mut resume_failures = history["_resume_failures"].as_u64().unwrap_or_default();
		while let Some(line) = lines.next_line().await.unwrap() {
			let request: Value = serde_json::from_str(&line).unwrap();
			sent.send(request.clone()).unwrap();
			if request.get("method").is_none() {
				continue;
			}
			if request["method"] == "thread/resume" && resume_failures > 0 {
				resume_failures -= 1;
				let mut frame = json!({"id":request["id"],"error":{"code":-32600,"message":"thread private-id already has an active writer"}}).to_string();
				frame.push('\n');
				writer.write_all(frame.as_bytes()).await.unwrap();
				continue;
			}
			if request["method"] == "turn/steer" && history["_steer_disconnect"] == true {
				break;
			}
			if request["method"] == "turn/steer" && history["_steer_error"] == true {
				let frame = format!(
					"{}\n",
					json!({"id":request["id"],"error":{"code":-32600,"message":"turn ended"}})
				);
				writer.write_all(frame.as_bytes()).await.unwrap();
				continue;
			}
			let result = match request["method"].as_str() {
				Some("turn/steer") => json!({"turnId":request["params"]["expectedTurnId"]}),
				Some("thread/read") => {
					let id = request["params"]["threadId"].as_str().unwrap();
					history.get(id).cloned().unwrap_or_else(
						|| json!({"thread":{"id":id,"turns":[],"status":{"type":"idle"}}}),
					)
				},
				Some("thread/resume") => {
					json!({"thread":{"id":request["params"]["threadId"],"turns":history[request["params"]["threadId"].as_str().unwrap()]["thread"]["turns"]},"model":"selected-model","reasoningEffort":request["params"]["config"]["model_reasoning_effort"]})
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
	coordinator.record_terminal(json!({"threadId":work.codex_thread_id,"turn":{"id":work.active_turn_id,"status":"completed","items":[]}}),Ok(json!({})),true).await.unwrap();
	let pending = coordinator.store.list_pending_chief_events(100).await.unwrap();
	assert_eq!(pending.len(), 1);
	assert!(pending[0].payload.contains("A later request"));
	assert!(pending[0].delivered_turn_id.is_none());
	let starts: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok())
		.filter(|request| request["method"] == "turn/start")
		.collect();
	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["input"][0]["text"], "Handle my actual request");
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

#[tokio::test]
async fn live_output_is_turn_bound_bounded_and_replaced_by_final_history() {
	let (mut coordinator, _sent, _directory) = fixture().await;
	let work = coordinator.start_chief("chief", "Talk").await.unwrap();
	let event = |turn: &str, text: &str| ServerEvent::Notification {
		method: "item/agentMessage/delta".into(),
		params: json!({"threadId":work.codex_thread_id,"turnId":turn,"itemId":"answer","delta":text}),
	};
	let turn = work.active_turn_id.as_deref().unwrap();
	coordinator.handle_event(event("old-turn", "wrong")).await.unwrap();
	assert!(coordinator.store.read_chief_output("chief".into()).await.unwrap().is_empty());
	coordinator.handle_event(event(turn, "Hello ")).await.unwrap();
	coordinator.handle_event(event(turn, "世界")).await.unwrap();
	let live = coordinator.store.read_chief_output("chief".into()).await.unwrap();
	assert_eq!(live[0].text, "Hello 世界");
	assert!(
		coordinator
			.store
			.list_pending_chief_events(100)
			.await
			.unwrap()
			.iter()
			.all(|event| event.event_kind != "assistant_delta")
	);
	coordinator.handle_event(event(turn, &"界".repeat(30000))).await.unwrap();
	let live = coordinator.store.read_chief_output("chief".into()).await.unwrap();
	assert!(live[0].truncated);
	assert!(live[0].text.len() <= 65536);
	complete(&mut coordinator, "chief").await;
	assert!(coordinator.store.read_chief_output("chief".into()).await.unwrap().is_empty());
}

#[tokio::test]
async fn nested_managers_own_their_inbox_tools_and_workspace_directory() {
	let (mut coordinator, mut sent, directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	let manager = coordinator
		.create_manager(
			"chief",
			"project",
			"Manage project",
			Some(("Project".into(), directory.path().display().to_string())),
		)
		.await
		.unwrap();
	coordinator.create_manager("project", "team", "Manage team", None).await.unwrap();
	coordinator.create_worker("team", "worker", "Do work").await.unwrap();
	let root = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let denied = coordinator.tool(&root,&json!({"tool":"chief_continue_worker","arguments":{"id":"worker","prompt":"Bypass its manager"}})).await;
	assert!(denied.is_err());
	let visible = coordinator
		.tool(&manager, &json!({"tool":"chief_list_work","arguments":{}}))
		.await
		.unwrap();
	assert!(
		visible["work"]
			.as_array()
			.unwrap()
			.iter()
			.all(|work| work["id"] != "chief" && work["id"] != "worker")
	);
	complete(&mut coordinator, "worker").await;
	assert!(
		coordinator.store.list_chief_wake_events("chief".into(), 100).await.unwrap().is_empty()
	);
	assert!(
		coordinator.store.list_chief_wake_events("project".into(), 100).await.unwrap().is_empty()
	);
	let events = coordinator.store.list_chief_wake_events("team".into(), 100).await.unwrap();
	assert_eq!(events.len(), 1);
	assert_eq!(events[0].work_item_id, "worker");
	let requests: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
	let starts: Vec<_> =
		requests.iter().filter(|request| request["method"] == "thread/start").collect();
	assert_eq!(starts.len(), 4);
	assert!(starts[1]["params"]["dynamicTools"].is_array());
	assert!(starts[2]["params"]["dynamicTools"].is_array());
	assert!(starts[3]["params"].get("dynamicTools").is_none());
	assert_eq!(starts[3]["params"]["cwd"], json!(directory.path().canonicalize().unwrap()));
}

#[tokio::test]
async fn legacy_manager_upgrades_tools_once_without_replaying_saved_input() {
	let (mut coordinator, mut sent, directory) = fixture().await;
	let original = coordinator.start_chief("chief", "Original request").await.unwrap();
	complete(&mut coordinator, "chief").await;
	coordinator
		.store
		.enqueue_chief_event(EnqueueChiefEvent {
			source_event_id: "old-user".into(),
			work_item_id: "chief".into(),
			event_kind: "user_message".into(),
			payload: json!({"text":"Remember the existing project"}).to_string(),
		})
		.await
		.unwrap();
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let db = rusqlite::Connection::open(root.paths().product_database_file()).unwrap();
	db.execute("DELETE FROM chief_tool_versions WHERE work_id='chief'", []).unwrap();
	while sent.try_recv().is_ok() {}
	coordinator.continue_worker("chief", "One new request").await.unwrap();
	let upgraded = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	assert_ne!(original.codex_thread_id, upgraded.codex_thread_id);
	assert_eq!(coordinator.store.chief_tool_version("chief".into()).await.unwrap(), 2);
	let messages: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
	assert_eq!(messages.iter().filter(|message| message["method"] == "thread/start").count(), 1);
	assert_eq!(messages.iter().filter(|message| message["method"] == "turn/start").count(), 1);
	let creation = messages.iter().find(|message| message["method"] == "thread/start").unwrap();
	assert!(
		creation["params"]["developerInstructions"]
			.as_str()
			.unwrap()
			.contains("Remember the existing project")
	);
	let start = messages.iter().find(|message| message["method"] == "turn/start").unwrap();
	assert_eq!(start["params"]["input"][0]["text"], "One new request");
	assert_eq!(
		db.query_row("SELECT count(*) FROM chief_thread_revisions", [], |row| row.get::<_, i64>(0))
			.unwrap(),
		1
	);
}

#[tokio::test]
async fn queued_user_messages_keep_native_turn_boundaries() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	ChiefCoordinator::reserve_root(&coordinator.store, "chief", "Chief").await.unwrap();
	coordinator
		.enqueue_user_message("chief", "first", "First message\nwith a second line")
		.await
		.unwrap();
	coordinator.enqueue_user_message("chief", "second", "Second message").await.unwrap();
	let evidence = coordinator
		.store
		.enqueue_chief_event(EnqueueChiefEvent {
			source_event_id: "background-result".into(),
			work_item_id: "chief".into(),
			event_kind: "automation_result".into(),
			payload: json!({"result":"Background evidence"}).to_string(),
		})
		.await
		.unwrap();
	coordinator.wake_pending().await.unwrap();
	let starts: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok())
		.filter(|request| request["method"] == "turn/start")
		.collect();
	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["input"][0]["text"], "First message\nwith a second line");
	complete(&mut coordinator, "chief").await;
	coordinator.wake_pending().await.unwrap();
	let starts: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok())
		.filter(|request| request["method"] == "turn/start")
		.collect();
	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["input"][0]["text"], "Second message");
	assert!(
		coordinator
			.store
			.get_chief_inbox_event(evidence.id)
			.await
			.unwrap()
			.delivered_turn_id
			.is_none()
	);
	let message = wake_message(&[evidence]).unwrap();
	assert!(message.contains("Background evidence"));
	assert!(!message.contains("source_event_id"));
	assert!(!message.contains("delivered_turn_id"));
}

#[tokio::test]
async fn usage_is_source_bound_persistent_and_does_not_wake_managers() {
	let (mut coordinator, _sent, directory) = fixture().await;
	let work = coordinator.start_chief("chief", "Talk").await.unwrap();
	let event = |turn: &str, input: i64| ServerEvent::Notification {
		method: "thread/tokenUsage/updated".into(),
		params: json!({"threadId":work.codex_thread_id,"turnId":turn,"tokenUsage":{
			"total":{"inputTokens":input,"outputTokens":45},
			"last":{"totalTokens":1200},"modelContextWindow":10000}}),
	};
	let turn = work.active_turn_id.as_deref().unwrap();
	let baseline = coordinator.store.read_chief_usage("chief".into()).await.unwrap();
	coordinator.handle_event(event("wrong-turn", 300)).await.unwrap();
	assert_eq!(coordinator.store.read_chief_usage("chief".into()).await.unwrap(), baseline);
	coordinator.handle_event(event(turn, 300)).await.unwrap();
	coordinator.handle_event(event(turn, -1)).await.unwrap();
	let saved = coordinator.store.read_chief_usage("chief".into()).await.unwrap().unwrap();
	let usage: Value = serde_json::from_str(&saved).unwrap();
	assert_eq!(usage["input_tokens"], 300);
	assert_eq!(usage["output_tokens"], 45);
	assert_eq!(usage["context_tokens"], 1200);
	assert_eq!(usage["context_window"], 10000);
	assert!(coordinator.store.list_pending_chief_events(100).await.unwrap().is_empty());
	complete(&mut coordinator, "chief").await;
	coordinator.handle_event(event(turn, 999)).await.unwrap();
	let paths =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap()
			.paths();
	drop(coordinator);
	let reopened = SqliteStore::open(&paths).unwrap();
	assert_eq!(reopened.read_chief_usage("chief".into()).await.unwrap(), Some(saved));
}

#[tokio::test]
async fn external_writer_keeps_input_unclaimed_until_exact_thread_can_resume() {
	let (mut coordinator, mut sent, _directory) =
		fixture_with_history(json!({"_resume_failures":1})).await;
	let original = coordinator.start_chief("chief", "Initial").await.unwrap();
	complete(&mut coordinator, "chief").await;
	coordinator.loaded_threads.clear();
	while sent.try_recv().is_ok() {}
	coordinator.enqueue_user_message("chief", "one-input", "Continue").await.unwrap();
	assert!(matches!(coordinator.wake_pending().await, Err(ChiefError::ThreadOwnedElsewhere)));
	let before = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	assert_eq!(before.dispatch_state, decodex_database::ChiefDispatchState::Idle);
	let pending = coordinator.store.list_chief_wake_events("chief".into(), 10).await.unwrap();
	assert_eq!(pending.len(), 1);
	assert!(pending[0].delivered_turn_id.is_none());
	coordinator.wake_pending().await.unwrap();
	let mut starts = 0;
	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "thread/start");
		if request["method"] == "turn/start" {
			starts += 1;
			assert_eq!(request["params"]["threadId"].as_str(), original.codex_thread_id.as_deref());
		}
	}
	assert_eq!(starts, 1);
}

#[tokio::test]
async fn turn_usage_sums_model_calls_without_double_counting_or_using_context_as_total() {
	let (mut coordinator, _sent, _directory) = fixture().await;
	let first = coordinator.start_chief("chief", "First").await.unwrap();
	let thread = first.codex_thread_id.clone().unwrap();
	let event = |turn: &str, input: i64, output: i64, context: i64| ServerEvent::Notification {
		method: "thread/tokenUsage/updated".into(),
		params: json!({"threadId":thread,"turnId":turn,"tokenUsage":{
			"total":{"inputTokens":input,"outputTokens":output},
			"last":{"totalTokens":context},"modelContextWindow":10000}}),
	};
	let turn = first.active_turn_id.as_deref().unwrap();
	coordinator.handle_event(event(turn, 600, 40, 640)).await.unwrap();
	coordinator.handle_event(event(turn, 1000, 100, 500)).await.unwrap();
	coordinator.handle_event(event(turn, 1000, 100, 500)).await.unwrap();
	complete(&mut coordinator, "chief").await;
	let events = coordinator.store.read_chief_work_events("chief".into(), 10).await.unwrap();
	let first_usage: Value = serde_json::from_str(&events.last().unwrap().payload).unwrap();
	assert_eq!(first_usage["usage"], json!({"input_tokens":1000,"output_tokens":100}));
	coordinator.continue_worker("chief", "Second").await.unwrap();
	let second = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let turn = second.active_turn_id.as_deref().unwrap();
	coordinator.handle_event(event(turn, 1300, 150, 250)).await.unwrap();
	coordinator.handle_event(event(turn, 1300, 150, 250)).await.unwrap();
	assert_eq!(
		coordinator.store.read_chief_turn_usage(thread.clone(), turn.into()).await.unwrap(),
		Some((300, 50))
	);
	let context: Value = serde_json::from_str(
		&coordinator.store.read_chief_usage("chief".into()).await.unwrap().unwrap(),
	)
	.unwrap();
	assert_eq!(context["context_tokens"], 250);
	complete(&mut coordinator, "chief").await;
	let events = coordinator.store.read_chief_work_events("chief".into(), 10).await.unwrap();
	let second_usage: Value = serde_json::from_str(&events.last().unwrap().payload).unwrap();
	assert_eq!(second_usage["usage"], json!({"input_tokens":300,"output_tokens":50}));
	coordinator.continue_worker("chief", "Third").await.unwrap();
	let third = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let turn = third.active_turn_id.as_deref().unwrap();
	coordinator.handle_event(event(turn, 10, 5, 15)).await.unwrap();
	coordinator.handle_event(event(turn, 2000, 200, 200)).await.unwrap();
	assert!(
		coordinator
			.store
			.read_chief_turn_usage(thread.clone(), turn.into())
			.await
			.unwrap()
			.is_none()
	);
}

#[tokio::test]
async fn native_usage_replay_restores_context_and_the_next_turn_baseline() {
	let (mut coordinator, _sent, _directory) = fixture_with_history(json!({"opaque thread/1":{"thread":{"turns":[{"id":"opaque turn/1","status":"completed","items":[]}]}}})).await;
	let first = coordinator.start_chief("chief", "Initial").await.unwrap();
	complete(&mut coordinator, "chief").await;
	coordinator.loaded_threads.clear();
	coordinator.continue_worker("chief", "Next").await.unwrap();
	let current = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	let thread = current.codex_thread_id.clone().unwrap();
	let event = |turn: &str, input: i64, output: i64, context: i64| ServerEvent::Notification {
		method: "thread/tokenUsage/updated".into(),
		params: json!({"threadId":thread,"turnId":turn,"tokenUsage":{
			"total":{"inputTokens":input,"outputTokens":output},
			"last":{"totalTokens":context},"modelContextWindow":10000}}),
	};
	coordinator
		.handle_event(event(first.active_turn_id.as_deref().unwrap(), 1000, 100, 700))
		.await
		.unwrap();
	let restored: Value = serde_json::from_str(
		&coordinator.store.read_chief_usage("chief".into()).await.unwrap().unwrap(),
	)
	.unwrap();
	assert_eq!(restored["context_tokens"], 700);
	coordinator
		.handle_event(event(current.active_turn_id.as_deref().unwrap(), 1300, 150, 450))
		.await
		.unwrap();
	coordinator
		.handle_event(event(first.active_turn_id.as_deref().unwrap(), 1000, 100, 700))
		.await
		.unwrap();
	assert_eq!(
		coordinator
			.store
			.read_chief_turn_usage(thread, current.active_turn_id.unwrap())
			.await
			.unwrap(),
		Some((300, 50))
	);
}

#[tokio::test]
async fn configured_message_dispatches_native_images_and_exact_turn_settings_once() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	ChiefCoordinator::reserve_root(&coordinator.store, "chief", "Personal Chief").await.unwrap();
	coordinator.store.enqueue_chief_event(EnqueueChiefEvent {
        source_event_id:"configured-message".into(),work_item_id:"chief".into(),event_kind:"user_message".into(),
        payload:json!({"text":"Inspect these files", "options":{
            "execution":{"model":"different-model","reasoning_effort":"high","fast":true},
            "attachments":[{"path":"/tmp/example.png","image":true},{"path":"/tmp/example.rs","image":false}]}}).to_string(),
    }).await.unwrap();
	coordinator.wake_pending().await.unwrap();
	coordinator.wake_pending().await.unwrap();
	let starts: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok())
		.filter(|r| r["method"] == "turn/start")
		.collect();
	assert_eq!(starts.len(), 1);
	let params = &starts[0]["params"];
	assert_eq!(params["model"], "different-model");
	assert_eq!(params["effort"], "high");
	assert_eq!(params["serviceTier"], "priority");
	assert_eq!(params["input"][0]["text"], "Inspect these files");
	assert_eq!(params["input"][1], json!({"type":"localImage","path":"/tmp/example.png"}));
	assert!(params["input"][2]["text"].as_str().unwrap().contains("/tmp/example.rs"));
	let mut params = json!({"input":[],"serviceTier":"priority"});
	apply_message_options(&mut params,&json!({"options":{"execution":{"model":"selected-model","reasoning_effort":"medium","fast":false},"attachments":[]}}).to_string()).unwrap();
	assert!(params["serviceTier"].is_null());
}

#[tokio::test]
async fn steer_uses_exact_running_turn_and_never_starts_a_second_turn() {
	let (mut chief, mut sent, _dir) = fixture().await;
	chief.start_chief("chief", "Initial task").await.unwrap();
	let work = chief.store.get_chief_work_item("chief".into()).await.unwrap();
	let turn = work.active_turn_id.unwrap();
	while sent.try_recv().is_ok() {}
	assert!(chief.steer_work("chief", "stale", "stale-command", "Supplement", &[]).await.is_err());
	assert!(sent.try_recv().is_err());
	chief.steer_work("chief", &turn, "steer-command", "Supplement", &[]).await.unwrap();
	let request = sent.recv().await.unwrap();
	assert_eq!(request["method"], "turn/steer");
	assert_eq!(request["params"]["expectedTurnId"], turn);
	assert_eq!(request["params"]["input"][0]["text"], "Supplement");
	assert_eq!(request["params"]["threadId"], work.codex_thread_id.unwrap());
	assert_eq!(request["params"]["clientUserMessageId"], "steer-command");
	assert!(chief.steer_work("chief", &turn, "steer-command", "Supplement", &[]).await.is_err());
	assert!(sent.try_recv().is_err());
	let receipts = chief.store.list_chief_events_for_turn(turn.clone(), 100).await.unwrap();
	assert!(
		receipts.iter().any(|e| e.event_kind == "user_message" && e.payload.contains("Supplement"))
	);
	assert!(chief.interrupt_work("chief", "stale").await.is_err());
	chief.interrupt_work("chief", &turn).await.unwrap();
	assert_eq!(sent.recv().await.unwrap()["method"], "turn/interrupt");
	complete(&mut chief, "chief").await;
	chief.wake_pending().await.unwrap();
	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start");
	}
}

#[tokio::test]
async fn rejected_or_uncertain_steer_never_replays_as_queued_input() {
	for flags in [json!({"_steer_error":true}), json!({"_steer_disconnect":true})] {
		let (mut chief, mut sent, _dir) = fixture_with_history(flags).await;
		chief.start_chief("chief", "Initial task").await.unwrap();
		let turn =
			chief.store.get_chief_work_item("chief".into()).await.unwrap().active_turn_id.unwrap();
		while sent.try_recv().is_ok() {}
		assert!(chief.steer_work("chief", &turn, "one-attempt", "Supplement", &[]).await.is_err());
		assert!(chief.store.list_undelivered_chief_events(100).await.unwrap().is_empty());
		assert!(chief.steer_work("chief", &turn, "one-attempt", "Supplement", &[]).await.is_err());
		assert_eq!(sent.recv().await.unwrap()["method"], "turn/steer");
		assert!(sent.try_recv().is_err());
	}
}

#[test]
fn steering_receipt_preserves_turn_settings_when_carried_as_evidence() {
	let mut params =
		json!({"input":[],"model":"current-model","effort":"high","serviceTier":"priority"});
	apply_message_options(&mut params,&json!({"text":"Supplement","options":{"attachments":[{"path":"/tmp/steer.png","image":true}]}}).to_string()).unwrap();
	assert_eq!(params["model"], "current-model");
	assert_eq!(params["effort"], "high");
	assert_eq!(params["serviceTier"], "priority");
	assert_eq!(params["input"][0]["type"], "localImage");
}

#[tokio::test]
async fn native_activity_notifications_reach_history_without_agent_delivery() {
	let (mut coordinator, _sent, _directory) = fixture().await;
	let work = coordinator.start_chief("chief", "Talk").await.unwrap();
	for method in ["item/started", "item/completed"] {
		coordinator
			.handle_event(ServerEvent::Notification {
				method: method.into(),
				params: json!({"threadId":work.codex_thread_id,"turnId":work.active_turn_id,
				"item":{"id":"compact", "type":"contextCompaction"}}),
			})
			.await
			.unwrap();
	}
	let (events, _) =
		coordinator.store.read_chief_transcript("chief".into(), None, 32).await.unwrap();
	let receipts: Vec<_> =
		events.iter().filter(|event| event.event_kind.starts_with("activity_")).collect();
	assert_eq!(receipts.len(), 1);
	let activity: decodex_protocol::ChiefActivityDto =
		serde_json::from_str(&receipts[0].payload).unwrap();
	assert_eq!(activity.label, "Compacting context");
	assert_eq!(activity.status, "completed");
	assert_eq!(receipts[0].disposition, Some(ChiefDisposition::Resolved));
}
