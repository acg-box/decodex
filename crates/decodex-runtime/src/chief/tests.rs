use super::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[path = "tests/archive.rs"] mod archive;
#[path = "tests/async_recovery.rs"] mod async_recovery;
#[path = "tests/auth_recovery.rs"] mod auth_recovery;
#[path = "tests/capacity.rs"] mod capacity;
#[path = "tests/guardian.rs"] mod guardian;
#[path = "tests/install.rs"] mod install;
#[path = "tests/native_goals.rs"] mod native_goals;
#[path = "tests/native_mcp_forms.rs"] mod native_mcp_forms;
#[path = "tests/native_permissions.rs"] mod native_permissions;
#[path = "tests/native_subagent_live.rs"] mod native_subagent_live;
#[path = "tests/native_subagents.rs"] mod native_subagents;
#[path = "tests/native_task_references.rs"] mod native_task_references;
#[path = "tests/task_history.rs"] mod task_history;

#[tokio::test]
async fn persistent_effort_survives_coordinator_admission_and_dispatch() {
	let (fixture, mut sent, _directory) = fixture().await;
	let mut config = fixture.config.clone();
	config.chief_effort = "persistent".into();
	config.worker_effort = "persistent".into();
	let mut chief =
		ChiefCoordinator::new(fixture.store.clone(), fixture.client.clone(), config.clone())
			.unwrap();
	chief.start_chief("chief", "Coordinate").await.unwrap();
	let mut starts = 0;
	while let Ok(request) = sent.try_recv() {
		match request["method"].as_str() {
			Some("thread/start") => {
				assert_eq!(request["params"]["config"]["model_reasoning_effort"], "persistent");
				starts += 1;
			},
			Some("turn/start") => {
				assert_eq!(request["params"]["effort"], "persistent");
				starts += 1;
			},
			_ => {},
		}
	}
	assert_eq!(starts, 2);
	for field in [&mut config.chief_effort, &mut config.worker_effort] {
		*field = "not-an-effort".into();
	}
	assert!(ChiefCoordinator::new(fixture.store, fixture.client, config).is_err());
}

#[tokio::test]
async fn subagent_activity_survives_parent_completion_and_restart_without_waking_work() {
	let (mut chief, mut sent, directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	let (io, mut write) = tokio::io::duplex(8192);
	let (read, writer) = tokio::io::split(io);
	let (_client, mut events) = AppServerClient::from_io(read, writer);
	for (index, kind) in ["started", "interacted", "interrupted", "completed"].iter().enumerate() {
		if *kind == "completed" {
			chief.handle_event(ServerEvent::Notification { method:"turn/completed".into(), params:json!({"threadId":"opaque thread/1","turn":{"id":"opaque turn/1","status":"completed","items":[]}})}).await.unwrap();
		}
		for (thread, turn) in [
			("foreign", "opaque turn/1"),
			("opaque thread/1", "unknown"),
			("opaque thread/1", "opaque turn/1"),
		] {
			for method in ["item/started", "item/completed", "item/completed"] {
				let wire = json!({"method":method,"params":{"threadId":thread,"turnId":turn,"item":{"id":format!("activity-{index}"),"type":"subAgentActivity","kind":kind,"agentThreadId":"child-thread","agentPath":"/root/worker","prompt":"PRIVATE_PROMPT"}}});
				write.write_all(format!("{wire}\n").as_bytes()).await.unwrap();
				let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
					.await
					.unwrap()
					.unwrap();
				chief.handle_event(event).await.unwrap();
			}
		}
	}
	while sent.try_recv().is_ok() {}
	chief.wake_pending().await.unwrap();
	assert!(sent.try_recv().is_err());
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	drop(chief);
	let store = SqliteStore::open(&root.paths()).unwrap();
	let (history, _) = store.read_chief_transcript("chief".into(), None, 32).await.unwrap();
	let activities: Vec<decodex_protocol::ChiefActivityDto> = history
		.iter()
		.filter(|e| e.event_kind == "activity_completed")
		.map(|e| serde_json::from_str(&e.payload).unwrap())
		.collect();
	assert_eq!(activities.len(), 4);
	assert_eq!(
		activities.iter().map(|a| a.label.as_str()).collect::<Vec<_>>(),
		[
			"Subagent started",
			"Message sent to subagent",
			"Subagent interrupted",
			"Subagent completed a turn"
		]
	);
	assert!(activities.iter().all(|a| a.status == "completed"
		&& a.detail == "/root/worker"
		&& a.turn_id == "opaque turn/1"));
	assert!(history.iter().all(|e| !e.payload.contains("PRIVATE_PROMPT")));
	assert!(store.list_chief_wake_events("chief".into(), 32).await.unwrap().is_empty());
	let work = store.get_chief_work_item("chief".into()).await.unwrap();
	assert_eq!(work.dispatch_state, decodex_database::ChiefDispatchState::Idle);
	assert_eq!(work.status, decodex_database::ChiefWorkStatus::Open);
}

#[tokio::test]
async fn terminal_readback_recovers_missed_subagent_activity() {
	let history = json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","status":{"type":"idle"},"turns":[{"id":"opaque turn/1","status":"completed","items":[{"id":"recovered","type":"subAgentActivity","kind":"started","agentThreadId":"child","agentPath":"/root/worker"}]}]}}});
	let (mut chief, _sent, _directory) = fixture_with_history(history).await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	chief.recover_persisted().await.unwrap();
	let (history, _) = chief.store.read_chief_transcript("chief".into(), None, 32).await.unwrap();
	let events: Vec<_> = history.iter().filter(|e| e.event_kind == "activity_completed").collect();
	assert_eq!(events.len(), 1);
	assert!(events[0].payload.contains("Subagent started"));
}

#[tokio::test]
async fn strict_review_notice_is_turn_bound_and_does_not_wake_or_stop_execution() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let (read, mut write) = tokio::io::duplex(8192);
	let (reader, writer) = tokio::io::split(read);
	let (_client, mut events) = AppServerClient::from_io(reader, writer);
	for (thread, turn, started) in [
		("wrong", "opaque turn/1", json!(1)),
		("opaque thread/1", "old", json!(1)),
		("opaque thread/1", "opaque turn/1", json!(-1)),
		("opaque thread/1", "opaque turn/1", json!("1")),
		("opaque thread/1", "opaque turn/1", json!(1)),
		("opaque thread/1", "opaque turn/1", json!(2)),
	] {
		let wire = json!({"method":"autoApprovalReview/strictReviewRequired","params":{"threadId":thread,"turnId":turn,"startedAtMs":started}});
		write.write_all(format!("{wire}\n").as_bytes()).await.unwrap();
		let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
			.await
			.unwrap()
			.unwrap();
		coordinator.handle_event(event).await.unwrap();
	}
	let (history, _) =
		coordinator.store.read_chief_transcript("chief".into(), None, 32).await.unwrap();
	assert_eq!(
		history.iter().filter(|event| event.event_kind == "strict_review_notice").count(),
		1
	);
	assert!(coordinator.store.list_pending_chief_events(32).await.unwrap().is_empty());
	assert!(coordinator.store.list_chief_wake_events("chief".into(), 32).await.unwrap().is_empty());
	assert_eq!(
		coordinator.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		decodex_database::ChiefDispatchState::Running
	);
	coordinator.wake_pending().await.unwrap();
	assert!(sent.try_recv().is_err());
}

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
	assert_eq!(history.iter().filter(|event| event.event_kind == "assistant_message").count(), 1);
	assert_eq!(history.iter().filter(|event| event.event_kind == "context_compacted").count(), 1);
	assert_eq!(history.iter().filter(|event| event.event_kind == "activity_completed").count(), 1);
	assert!(coordinator.store.read_chief_usage("chief".into()).await.unwrap().is_some());
	assert!(coordinator.store.list_pending_chief_events(10).await.unwrap().is_empty());
	coordinator.wake_pending().await.unwrap();
	assert!(sent.try_recv().is_err());
	coordinator.recover_persisted().await.unwrap();
	let usage = coordinator
		.store
		.read_chief_usage_observation(
			"chief".into(),
			"opaque thread/1".into(),
			"opaque turn/1".into(),
		)
		.await
		.unwrap()
		.unwrap();
	assert_eq!(
		serde_json::from_str::<Value>(&usage.payload).unwrap()["tokenUsage"]["last"]["inputTokens"],
		1000
	);
}

#[path = "tests/external_context.rs"] mod external_context;
#[path = "tests/inbox_carryover.rs"] mod inbox_carryover;

#[path = "tests/result_integrity.rs"] mod result_integrity;

async fn fixture()
-> (ChiefCoordinator, tokio::sync::mpsc::UnboundedReceiver<Value>, tempfile::TempDir) {
	fixture_with_history(json!({})).await
}

pub(super) async fn fixture_with_history(
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
	let (client, mut events) = AppServerClient::from_io(reader, writer);
	tokio::spawn(async move { while events.recv().await.is_some() {} });
	let (sent, received) = tokio::sync::mpsc::unbounded_channel();
	tokio::spawn(serve_fixture(server_io, history, sent));
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

struct FixtureFaults {
	resume_failures: u64,
	archived: bool,
	injected: bool,
}
impl FixtureFaults {
	async fn respond(
		&mut self,
		request: &Value,
		history: &Value,
		writer: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>,
	) -> Option<bool> {
		if request["method"] == "thread/resume" && self.resume_failures > 0 {
			self.resume_failures -= 1;
			let mut frame = json!({"id":request["id"],"error":{"code":-32600,"message":"thread private-id already has an active writer"}}).to_string();
			frame.push('\n');
			writer.write_all(frame.as_bytes()).await.unwrap();
			return Some(true);
		}
		if request["method"] == "thread/realtime/stop" && history["_voice_stop_disconnect"] == true
		{
			return Some(false);
		}
		if request["method"] == "thread/unarchive" {
			if history["_archive_disconnect"] == true {
				return Some(false);
			}
			if history["_archive_reject"] == true {
				if history["_archive_peer_restored"] == true {
					self.archived = false;
				}
				writer
					.write_all(
						format!(
							"{}\n",
							json!({"id":request["id"],"error":{"code":-32600,"message":"restore rejected"}})
						)
						.as_bytes(),
					)
					.await
					.unwrap();
				return Some(true);
			}
		}
		if request["method"] == "thread/approveGuardianDeniedAction" {
			if history["_guardian_disconnect"] == true {
				return Some(false);
			}
			if history["_guardian_reject"] == true {
				let frame = format!(
					"{}\n",
					json!({"id":request["id"],"error":{"code":-32600,"message":"approval rejected"}})
				);
				writer.write_all(frame.as_bytes()).await.unwrap();
				return Some(true);
			}
		}
		if request["method"] == "thread/inject_items" && history["_injection_disconnect"] == true {
			return Some(false);
		}
		if request["method"] == "thread/inject_items" {
			self.injected = true;
		}
		if request["method"] == "turn/start"
			&& self.injected
			&& history["_turn_after_injection_disconnect"] == true
		{
			return Some(false);
		}
		if request["method"] == "turn/steer" && history["_steer_disconnect"] == true {
			return Some(false);
		}
		if request["method"] == "turn/start"
			&& request["params"]["toolOutput"].is_object()
			&& history["_tool_output_disconnect"] == true
		{
			return Some(false);
		}
		if request["method"] == "turn/steer" && history["_steer_error"] == true {
			let frame = format!(
				"{}\n",
				json!({"id":request["id"],"error":{"code":-32600,"message":"turn ended"}})
			);
			writer.write_all(frame.as_bytes()).await.unwrap();
			return Some(true);
		}
		if request["method"] == "turn/start"
			&& request["params"]["responsesapiClientMetadata"]["misalignment_override"].is_string()
		{
			if history["_continuation_disconnect"] == true {
				return Some(false);
			}
			if history["_continuation_reject"] == true {
				let frame = format!(
					"{}\n",
					json!({"id":request["id"],"error":{"code":-32600,"message":"continuation rejected"}})
				);
				writer.write_all(frame.as_bytes()).await.unwrap();
				return Some(true);
			}
		}
		None
	}
}

async fn serve_fixture(
	server_io: tokio::io::DuplexStream,
	history: Value,
	sent: tokio::sync::mpsc::UnboundedSender<Value>,
) {
	let (reader, mut writer) = tokio::io::split(server_io);
	let mut lines = BufReader::new(reader).lines();
	let mut threads = 0;
	let mut turns = 0;

	let mut faults = FixtureFaults {
		injected: false,
		archived: history["_archived"] == true,
		resume_failures: history["_resume_failures"].as_u64().unwrap_or_default(),
	};
	while let Some(line) = lines.next_line().await.unwrap() {
		let request: Value = serde_json::from_str(&line).unwrap();
		sent.send(request.clone()).unwrap();
		if request.get("method").is_none() {
			continue;
		}
		match faults.respond(&request, &history, &mut writer).await {
			Some(true) => continue,
			Some(false) => break,
			None => {},
		}
		let result = match request["method"].as_str() {
			Some("thread/list") =>
				json!({"data":if request["params"]["archived"]==faults.archived {vec![json!({"id":"opaque thread/1"})]} else {vec![]},"nextCursor":null}),
			Some("thread/unarchive") => {
				faults.archived = false;
				json!({"thread":{"id":request["params"]["threadId"]}})
			},
			Some("turn/steer") => json!({"turnId":request["params"]["expectedTurnId"]}),
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
					if request["params"]["itemsView"] == "full" {
						turn["itemsView"] = json!("full");
					} else {
						turn["items"] = json!([]);
						turn["itemsView"] = json!("notLoaded");
					}
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
		if request["method"] == "thread/read" && history["_misalignment_revert_on_read"] == true {
			let notice =
				json!({"method":"thread/reverted","params":{"threadId":"opaque thread/1"}});
			writer.write_all(format!("{notice}\n").as_bytes()).await.unwrap();
		}
		if request["method"] == "turn/start"
			&& turns == 1
			&& history["_live_misalignment"].is_object()
		{
			let notification = json!({"method":"error","params":{"threadId":"opaque thread/1","turnId":"opaque turn/1","willRetry":false,"error":history["_live_misalignment"]}});
			writer.write_all(format!("{notification}\n").as_bytes()).await.unwrap();
		}
		let mut frame = json!({"id":request["id"],"result":result}).to_string();
		frame.push('\n');
		writer.write_all(frame.as_bytes()).await.unwrap();
	}
}

#[tokio::test]
async fn unloaded_thread_resumes_exact_identity_without_new_thread() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	complete(&mut coordinator, "chief").await;
	let original = coordinator.store.get_chief_work_item("chief".into()).await.unwrap();
	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "thread/resume");
		if request["method"] == "thread/start" {
			assert_eq!(request["params"]["approvalPolicy"], coordinator.config.approval_policy);
			assert_eq!(request["params"]["sandbox"], coordinator.config.sandbox);
			assert_eq!(request["params"]["cwd"], coordinator.config.cwd);
		}
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
	for field in ["approvalPolicy", "sandbox", "cwd", "dynamicTools"] {
		assert!(resumes[0]["params"].get(field).is_none(), "resume must preserve {field}");
	}
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
	let turn = requests.iter().find(|request| request["method"] == "turn/start").unwrap();
	assert_eq!(turn["params"]["input"], json!([]));
	assert_eq!(
		turn["params"]["toolOutput"],
		json!({"name":"work_instruction","namespace":"decodex","output":"Use the result"})
	);
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
	assert_eq!(coordinator.store.chief_tool_version("chief".into()).await.unwrap(), 3);
	let messages: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
	assert_eq!(messages.iter().filter(|message| message["method"] == "thread/start").count(), 1);
	assert_eq!(messages.iter().filter(|message| message["method"] == "turn/start").count(), 1);
	let creation = messages.iter().find(|message| message["method"] == "thread/start").unwrap();
	assert!(
		!creation["params"]["developerInstructions"]
			.as_str()
			.unwrap()
			.contains("Remember the existing project")
	);
	let injected =
		messages.iter().find(|message| message["method"] == "thread/inject_items").unwrap();
	assert_eq!(injected["params"]["threadId"], json!(upgraded.codex_thread_id));
	assert_eq!(injected["params"]["items"][0]["type"], "function_call_output");
	assert!(
		injected["params"]["items"][0]["output"]
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
	let message = wake_message(std::slice::from_ref(&evidence)).unwrap();
	assert!(!message.contains("Background evidence"));
	assert!(wake_evidence(&evidence).to_string().contains("Background evidence"));
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
	assert_eq!(params["serviceTierForTurn"], "priority");
	assert_eq!(params["input"][0]["text"], "Inspect these files");
	assert_eq!(params["input"][1], json!({"type":"localImage","path":"/tmp/example.png"}));
	assert!(params["input"][2]["text"].as_str().unwrap().contains("/tmp/example.rs"));
	let mut params = json!({"input":[],"serviceTier":"priority"});
	apply_message_options(&mut params,&json!({"options":{"execution":{"model":"selected-model","reasoning_effort":"medium","fast":false},"attachments":[]}}).to_string()).unwrap();
	assert!(params["serviceTier"].is_null());
	assert_eq!(params["serviceTierForTurn"], "default");
	let tiered = json!({"options":{"execution":{"model":"chosen","reasoning_effort":"high","fast":false,"service_tier":"ultrafast"},"attachments":[]}});
	apply_message_options(&mut params, &tiered.to_string()).unwrap();
	assert_eq!(params["serviceTier"], "ultrafast");
	assert_eq!(params["serviceTierForTurn"], "ultrafast");
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

#[tokio::test]
async fn native_revert_retires_exact_thread_requests_without_replies_or_replay() {
	let (mut chief, mut sent, _directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let id = RequestId::Number(73);
	chief.handle_event(ServerEvent::Request { id: id.clone(), method: "item/commandExecution/requestApproval".into(), params: json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","itemId":"item","command":"pwd"}) }).await.unwrap();
	let event_id = chief.pending_requests[&id];
	chief
		.handle_event(ServerEvent::Notification {
			method: "thread/reverted".into(),
			params: json!({"threadId":"other"}),
		})
		.await
		.unwrap();
	assert!(chief.pending_requests.contains_key(&id));
	for _ in 0..2 {
		chief
			.handle_event(ServerEvent::Notification {
				method: "thread/reverted".into(),
				params: json!({"threadId":"opaque thread/1"}),
			})
			.await
			.unwrap();
	}
	assert!(!chief.pending_requests.contains_key(&id));
	let event = chief.store.get_chief_inbox_event(event_id).await.unwrap();
	assert_eq!(event.disposition, Some(ChiefDisposition::Resolved));
	assert!(chief.respond_pending_event(event_id, json!({"decision":"accept"})).await.is_err());
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn native_request_resolution_requires_exact_thread_and_request_identity() {
	let (mut coordinator, mut sent, _directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	for id in [RequestId::String("shared-item-A".into()), RequestId::Number(7)] {
		coordinator.handle_event(ServerEvent::Request { id: id.clone(), method:"item/commandExecution/requestApproval".into(),params:json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","itemId":"shared-item","command":"pwd"}) }).await.unwrap();
	}
	let id = RequestId::String("shared-item-A".into());
	let event_id = coordinator.pending_requests[&id];
	for params in [
		json!({"threadId":"other-thread","requestId":id}),
		json!({"threadId":"opaque thread/1","requestId":"7"}),
		json!({"threadId":"opaque thread/1","requestId":null}),
	] {
		coordinator
			.handle_event(ServerEvent::Notification {
				method: "serverRequest/resolved".into(),
				params,
			})
			.await
			.unwrap();
	}
	assert_eq!(coordinator.pending_requests.len(), 2);
	for _ in 0..2 {
		coordinator
			.handle_event(ServerEvent::Notification {
				method: "serverRequest/resolved".into(),
				params: json!({"threadId":"opaque thread/1","requestId":id}),
			})
			.await
			.unwrap();
	}
	assert!(!coordinator.pending_requests.contains_key(&id));
	assert!(coordinator.pending_requests.contains_key(&RequestId::Number(7)));
	let event = coordinator.store.get_chief_inbox_event(event_id).await.unwrap();
	assert_eq!(event.disposition, Some(ChiefDisposition::Resolved));
	assert!(event.disposition_note.unwrap().contains("no local response was sent"));
	assert!(
		coordinator.respond_pending_event(event_id, json!({"decision":"accept"})).await.is_err()
	);
	assert!(sent.try_recv().is_err());
	assert_eq!(
		coordinator.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		decodex_database::ChiefDispatchState::Running
	);
}

#[tokio::test]
async fn async_question_answers_survive_replay_and_reopening_without_waking_work() {
	let (mut coordinator, mut sent, directory) = fixture().await;
	coordinator.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let first_id = decodex_protocol::chief_async_question_id("questions", 0);
	let reply = decodex_protocol::chief_async_question_reply(
		&decodex_protocol::ChiefAsyncQuestionDto {
			id: first_id.clone(),
			title: "Same".into(),
			options: vec![],
		},
		"A",
	)
	.unwrap();
	let response = json!({"type":"userMessage","id":"answer","content":[{"type":"text","text":reply.as_str()}]});
	let questions = json!({"type":"agentMessage","delivery":"async","id":"questions","text":"Choose","questions":[{"title":"Same","options":["A","B"]},{"title":"Same","options":["A","B"]}]});
	// A committed answer can arrive before the corresponding history item.
	for item in [response, questions.clone(), questions.clone()] {
		coordinator
			.handle_event(ServerEvent::Notification {
				method: "item/completed".into(),
				params: json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","item":item}),
			})
			.await
			.unwrap();
	}
	let pending = coordinator.store.read_chief_async_questions("chief".into()).await.unwrap();
	assert_eq!(pending.len(), 1);
	assert_eq!(pending[0].question_id, decodex_protocol::chief_async_question_id("questions", 1));
	coordinator
		.store
		.resolve_chief_async_questions("unowned".into(), vec![pending[0].question_id.clone()])
		.await
		.unwrap();
	assert_eq!(
		coordinator.store.read_chief_async_questions("chief".into()).await.unwrap().len(),
		1
	);
	let paths =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap()
			.paths();
	let reopened = SqliteStore::open(&paths).unwrap();
	assert_eq!(reopened.read_chief_async_questions("chief".into()).await.unwrap().len(), 1);
	// Older desktop replies identify the source message and resolve all its questions.
	reopened
		.resolve_chief_async_questions("opaque thread/1".into(), vec!["questions".into()])
		.await
		.unwrap();
	coordinator
		.observe_async_question_item("opaque thread/1", "opaque turn/1", &questions)
		.await
		.unwrap();
	assert!(reopened.read_chief_async_questions("chief".into()).await.unwrap().is_empty());
	assert!(coordinator.store.list_pending_chief_events(100).await.unwrap().is_empty());
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn async_question_upgrade_reads_native_history_and_preserves_later_questions() {
	let question = |id: &str| json!({"id":id,"type":"agentMessage","delivery":"async","text":"Question","questions":[{"title":"Which?","options":["A","B"]}]});
	let answer = decodex_protocol::chief_async_question_reply(
		&decodex_protocol::ChiefAsyncQuestionDto {
			id: decodex_protocol::chief_async_question_id("answered", 0),
			title: "Which?".into(),
			options: vec![],
		},
		"B",
	)
	.unwrap();
	let history = json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"old","status":"completed","items":[question("old"),{"type":"userMessage","content":[{"type":"text","text":"New task"}]},question("answered"),{"type":"userMessage","content":[{"type":"text","text":answer.as_str()}]},question("pending")]}]}}});
	let (mut chief, mut sent, directory) = fixture_with_history(history).await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let db = rusqlite::Connection::open(root.paths().product_database_file()).unwrap();
	db.execute(
		"INSERT INTO chief_async_recovery(work_id,thread_id) VALUES('chief','opaque thread/1')",
		[],
	)
	.unwrap();
	drop(db);
	chief.recover_async_questions().await.unwrap();
	let pending = chief.store.read_chief_async_questions("chief".into()).await.unwrap();
	assert_eq!(pending.len(), 1);
	assert_eq!(pending[0].item_id, "pending");
	assert!(chief.store.pending_chief_async_recovery().await.unwrap().is_empty());
	while let Ok(request) = sent.try_recv() {
		assert_eq!(request["method"], "thread/read");
	}
	chief.recover_async_questions().await.unwrap();
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn reverted_async_history_reopens_retained_questions_and_removes_deleted_questions() {
	let item = json!({"id":"question","type":"agentMessage","delivery":"async","text":"Question","questions":[{"title":"Which?"}]});
	let history = json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"old","status":"completed","items":[item.clone()]}]}}});
	let (mut chief, mut sent, _directory) = fixture_with_history(history).await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	chief.observe_async_question_item("opaque thread/1", "old", &item).await.unwrap();
	let id = decodex_protocol::chief_async_question_id("question", 0);
	chief
		.store
		.resolve_chief_async_questions("opaque thread/1".into(), vec![id.clone()])
		.await
		.unwrap();
	assert!(chief.store.read_chief_async_questions("chief".into()).await.unwrap().is_empty());
	while sent.try_recv().is_ok() {}
	chief
		.handle_event(ServerEvent::Notification {
			method: "thread/reverted".into(),
			params: json!({"threadId":"opaque thread/1"}),
		})
		.await
		.unwrap();
	assert!(chief.store.chief_async_questions_recovering("chief".into()).await.unwrap());
	chief.recover_async_questions().await.unwrap();
	let questions = chief.store.read_chief_async_questions("chief".into()).await.unwrap();
	assert_eq!(questions.len(), 1);
	assert_eq!(questions[0].question_id, id);
	while let Ok(request) = sent.try_recv() {
		assert_eq!(request["method"], "thread/read");
	}
	let queued = chief
		.store
		.enqueue_chief_event(EnqueueChiefEvent {
			source_event_id: "offline-answer".into(),
			work_item_id: "chief".into(),
			event_kind: "async_question_answer".into(),
			payload: json!({"text":"answer","asyncQuestionId":id}).to_string(),
		})
		.await
		.unwrap();
	let empty = json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[]}}});
	let (mut reconnected, mut sent, _other) = fixture_with_history(empty).await;
	reconnected.store = chief.store.clone();
	reconnected.store.queue_chief_async_reconnection().await.unwrap();
	reconnected.recover_async_questions().await.unwrap();
	assert!(reconnected.store.read_chief_async_questions("chief".into()).await.unwrap().is_empty());
	assert!(!reconnected.store.chief_async_questions_recovering("chief".into()).await.unwrap());
	assert_eq!(
		reconnected.store.get_chief_inbox_event(queued.id).await.unwrap().disposition,
		Some(ChiefDisposition::Resolved)
	);
	while let Ok(request) = sent.try_recv() {
		assert_eq!(request["method"], "thread/read");
	}
}

#[tokio::test]
async fn other_client_input_blocks_question_writes_before_owner_observation() {
	for ordinary in [false, true] {
		let (mut chief, mut sent, _directory) = fixture().await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		let item = json!({"id":"questions","type":"agentMessage","delivery":"async","questions":[{"title":"First?"},{"title":"Second?"}]});
		chief.observe_async_question_item("opaque thread/1", "opaque turn/1", &item).await.unwrap();
		let questions = decodex_protocol::project_chief_async_questions(&item).unwrap();
		let text = if ordinary {
			"New task".to_owned()
		} else {
			decodex_protocol::chief_async_question_reply(&questions[0], "A")
				.unwrap()
				.as_str()
				.to_owned()
		};
		while sent.try_recv().is_ok() {}
		let (incoming, frames) = tokio::sync::mpsc::channel(8);
		let (outgoing, mut writes) = tokio::sync::mpsc::channel(8);
		let (client, mut events) = AppServerClient::from_framed(1, frames, outgoing).unwrap();
		chief.client = client.clone();
		incoming.send(Ok(json!({"method":"item/completed","params":{"threadId":"opaque thread/1","turnId":"opaque turn/1","item":{"id":"input","type":"userMessage","content":[{"type":"text","text":text}]}}}))).await.unwrap();
		tokio::time::timeout(std::time::Duration::from_secs(2), async {
			while client.question_revision() == 0 {
				tokio::task::yield_now().await;
			}
		})
		.await
		.unwrap();
		assert!(
			chief
				.answer_async_question("chief", &questions[0].id, "B", "old-answer")
				.await
				.is_err()
		);
		assert!(writes.try_recv().is_err());
		assert!(sent.try_recv().is_err());
		assert_eq!(client.history_revision(), 0);
		if !ordinary {
			chief.handle_event(events.recv().await.unwrap()).await.unwrap();
			let pending = chief.store.read_chief_async_questions("chief".into()).await.unwrap();
			assert_eq!(pending.len(), 1);
			assert_eq!(pending[0].question_id, questions[1].id);
			assert!(client.question_guard(chief.handled_question_revision).is_some());
		}
	}
}

#[tokio::test]
async fn transport_revert_blocks_old_question_before_coordinator_reads_notification() {
	let (mut chief, mut sent, _directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	let question = json!({"id":"question","type":"agentMessage","delivery":"async","questions":[{"title":"Which?"}]});
	chief.observe_async_question_item("opaque thread/1", "opaque turn/1", &question).await.unwrap();
	while sent.try_recv().is_ok() {}
	let (incoming, frames) = tokio::sync::mpsc::channel(8);
	let (outgoing, mut writes) = tokio::sync::mpsc::channel(8);
	let (client, mut notifications) = AppServerClient::from_framed(1, frames, outgoing).unwrap();
	chief.client = client.clone();
	incoming
		.send(Ok(json!({"method":"thread/reverted","params":{"threadId":"opaque thread/1"}})))
		.await
		.unwrap();
	tokio::time::timeout(std::time::Duration::from_secs(2), async {
		while client.history_revision() == 0 {
			tokio::task::yield_now().await;
		}
	})
	.await
	.unwrap();
	let id = decodex_protocol::chief_async_question_id("question", 0);
	assert!(matches!(
		chief.answer_async_question("chief", &id, "A", "stale-ui").await,
		Err(super::ChiefError::Invalid(_))
	));
	assert!(writes.try_recv().is_err());
	assert!(sent.try_recv().is_err());
	assert_eq!(
		chief.store.read_chief_async_questions("chief".into()).await.unwrap().len(),
		1,
		"old projection exists until owner handles the notification"
	);
	chief.handle_event(notifications.recv().await.unwrap()).await.unwrap();
	assert!(chief.store.chief_async_questions_recovering("chief".into()).await.unwrap());
	assert!(chief.answer_async_question("chief", &id, "A", "after-owner").await.is_err());
	assert!(writes.try_recv().is_err());
}

#[tokio::test]
async fn stale_history_guard_prevents_async_turn_and_steer_without_unknown_receipts() {
	for running in [false, true] {
		let (mut chief, mut sent, directory) = fixture().await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		if !running {
			chief.handle_event(ServerEvent::Notification {method:"turn/completed".into(),params:json!({"threadId":"opaque thread/1","turn":{"id":"opaque turn/1","status":"completed","items":[]}})}).await.unwrap();
		}
		if !running {
			let root = decodex_core::DecodexRoot::new(
				directory.path().canonicalize().unwrap().join("root"),
			)
			.unwrap();
			let db = rusqlite::Connection::open(root.paths().product_database_file()).unwrap();
			db.execute("UPDATE chief_work_items SET status='wait',next_check_at_micros=9999999999999999 WHERE id='chief'",[]).unwrap();
		}
		let work = chief.store.get_chief_work_item("chief".into()).await.unwrap();
		let (foreign, _events, _process) = {
			let (io, remote) = tokio::io::duplex(4096);
			let (read, write) = tokio::io::split(io);
			let (client, events) = AppServerClient::from_io(read, write);
			(client, events, remote)
		};
		let guard = foreign.history_guard(0).unwrap();
		while sent.try_recv().is_ok() {}
		let result = if running {
			chief
				.steer_work_with_question_reply(
					"chief",
					"opaque turn/1",
					"stale-answer",
					"answer",
					super::ChiefInputExtras { attachments: &[], task_references: &[] },
					Some(("question", guard)),
				)
				.await
				.map(|_| String::new())
		} else {
			let event = chief
				.store
				.enqueue_chief_event(EnqueueChiefEvent {
					source_event_id: "stale-answer".into(),
					work_item_id: "chief".into(),
					event_kind: "async_question_answer".into(),
					payload: json!({"text":"answer","asyncQuestionId":"question"}).to_string(),
				})
				.await
				.unwrap();
			chief.dispatch_with_claim(&work, "answer", vec![event.id], None, Some(guard)).await
		};
		assert!(matches!(result, Err(super::ChiefError::Transport(ClientError::StaleHistory))));
		let after = chief.store.get_chief_work_item("chief".into()).await.unwrap();
		assert_eq!(after.dispatch_state, work.dispatch_state);
		assert_eq!(after.status, work.status);
		assert_eq!(after.next_check_at_micros, work.next_check_at_micros);
		assert!(
			!chief
				.store
				.chief_async_answer_pending("chief".into(), "question".into())
				.await
				.unwrap()
		);
		assert!(sent.try_recv().is_err());
	}
}

#[tokio::test]
async fn async_revert_marker_survives_reopen_and_preserves_uncertain_deliveries() {
	let (mut chief, mut sent, directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let mut ids = Vec::new();
	for kind in ["unsent", "unknown", "accepted", "ordinary"] {
		let event = chief
			.store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: format!("revert-{kind}"),
				work_item_id: "chief".into(),
				event_kind: if kind == "ordinary" {
					"user_message"
				} else {
					"async_question_answer"
				}
				.into(),
				payload: json!({"text":"answer","asyncQuestionId":"q"}).to_string(),
			})
			.await
			.unwrap();
		ids.push(event.id);
	}
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let db = rusqlite::Connection::open(root.paths().product_database_file()).unwrap();
	db.execute(
		"UPDATE chief_inbox_events SET delivered_turn_id='',delivery_work_item_id='chief' WHERE id=?1",
		[ids[1]],
	)
	.unwrap();
	db.execute(
		"UPDATE chief_inbox_events SET delivered_turn_id='accepted-turn',delivery_work_item_id='chief' WHERE id=?1",
		[ids[2]],
	)
	.unwrap();
	drop(db);
	chief.store.queue_chief_async_revert("unrelated".into()).await.unwrap();
	assert!(chief.store.get_chief_inbox_event(ids[0]).await.unwrap().disposition.is_none());
	chief.store.queue_chief_async_revert("opaque thread/1".into()).await.unwrap();
	let reopened = SqliteStore::open(&root.paths()).unwrap();
	assert!(reopened.chief_async_questions_recovering("chief".into()).await.unwrap());
	assert_eq!(
		reopened.get_chief_inbox_event(ids[0]).await.unwrap().disposition,
		Some(ChiefDisposition::Resolved)
	);
	for id in &ids[1..] {
		assert!(reopened.get_chief_inbox_event(*id).await.unwrap().disposition.is_none());
	}
	assert_eq!(
		reopened.get_chief_inbox_event(ids[1]).await.unwrap().delivered_turn_id.as_deref(),
		Some("")
	);
	assert!(reopened.chief_async_answer_pending("chief".into(), "q".into()).await.unwrap());
	reopened
		.request_chief_async_recovery("opaque thread/1".into(), "new-prompt".into())
		.await
		.unwrap();
	assert!(
		!reopened
			.replace_chief_async_projection(
				"chief".into(),
				"opaque thread/1".into(),
				None,
				vec![],
				vec![]
			)
			.await
			.unwrap()
	);
	assert!(reopened.chief_async_questions_recovering("chief".into()).await.unwrap());
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn incomplete_async_recovery_hides_cards_until_a_later_complete_read() {
	let bad =
		json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","historyMode":"unknown"}}});
	let (mut chief, mut sent, directory) = fixture_with_history(bad).await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	let item = json!({"id":"pending","type":"agentMessage","delivery":"async","text":"Question","questions":[{"title":"Which?"}]});
	chief.observe_async_question_item("opaque thread/1", "old", &item).await.unwrap();
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let db = rusqlite::Connection::open(root.paths().product_database_file()).unwrap();
	db.execute(
		"INSERT INTO chief_async_recovery(work_id,thread_id) VALUES('chief','opaque thread/1')",
		[],
	)
	.unwrap();
	drop(db);
	while sent.try_recv().is_ok() {}
	chief.recover_async_questions().await.unwrap();
	assert!(chief.store.chief_async_questions_recovering("chief".into()).await.unwrap());
	assert!(chief.store.read_chief_async_questions("chief".into()).await.unwrap().is_empty());
	while let Ok(request) = sent.try_recv() {
		assert_eq!(request["method"], "thread/read");
	}
	let good = json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","historyMode":"paginated","turns":[{"id":"old","status":"completed","items":[item]}]}}});
	let (mut recovered, mut sent, _other_directory) = fixture_with_history(good).await;
	recovered.store = chief.store.clone();
	recovered.recover_async_questions().await.unwrap();
	assert!(!recovered.store.chief_async_questions_recovering("chief".into()).await.unwrap());
	assert_eq!(recovered.store.read_chief_async_questions("chief".into()).await.unwrap().len(), 1);
	let mut saw_items = false;
	while let Ok(request) = sent.try_recv() {
		let method = request["method"].as_str().unwrap();
		assert!(["thread/read", "thread/turns/list", "thread/items/list"].contains(&method));
		saw_items |= method == "thread/items/list";
	}
	assert!(saw_items);
}

#[tokio::test]
async fn async_answers_target_running_and_idle_workers_and_preserve_sibling_questions() {
	for idle in [false, true] {
		let (mut chief, mut sent, _directory) = fixture().await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		let worker = chief.create_worker("chief", "worker", "Inspect").await.unwrap();
		let thread = worker.codex_thread_id.unwrap();
		let turn = worker.active_turn_id.unwrap();
		let item = json!({"id":"question","type":"agentMessage","delivery":"async","text":"Questions","questions":[{"title":"Same title"},{"title":"Same title"}]});
		chief.observe_async_question_item(&thread, &turn, &item).await.unwrap();
		if idle {
			chief.store.complete_chief_turn("worker".into(), turn.clone()).await.unwrap();
		}
		while sent.try_recv().is_ok() {}
		let question_id = decodex_protocol::chief_async_question_id("question", 1);
		chief
			.answer_async_question("worker", &question_id, "Explicit choice", "answer-1")
			.await
			.unwrap();
		let mut inputs = Vec::new();
		while let Ok(request) = sent.try_recv() {
			if request["method"] == "turn/start" || request["method"] == "turn/steer" {
				inputs.push(request);
			}
		}
		assert_eq!(inputs.len(), 1);
		assert_eq!(inputs[0]["method"], if idle { "turn/start" } else { "turn/steer" });
		assert_eq!(inputs[0]["params"]["threadId"], thread);
		if !idle {
			assert_eq!(inputs[0]["params"]["expectedTurnId"], turn);
		}
		let replies = decodex_protocol::parse_chief_async_question_replies(
			inputs[0]["params"]["input"][0]["text"].as_str().unwrap(),
		)
		.unwrap();
		assert_eq!(replies[0].question_item_id, question_id);
		assert_eq!(replies[0].answer, "Explicit choice");
		let pending = chief.store.read_chief_async_questions("worker".into()).await.unwrap();
		assert_eq!(pending.len(), 1);
		assert_eq!(
			pending[0].question_id,
			decodex_protocol::chief_async_question_id("question", 0)
		);
		assert!(
			chief
				.answer_async_question("worker", &question_id, "Explicit choice", "answer-1")
				.await
				.is_err()
		);
		chief.wake_pending().await.unwrap();
		assert!(sent.try_recv().is_err());
	}
}

#[tokio::test]
async fn rejected_or_uncertain_async_answers_keep_question_and_do_not_queue_retry() {
	for history in [json!({"_steer_error":true}), json!({"_steer_disconnect":true})] {
		let uncertain = history["_steer_disconnect"] == true;
		let (mut chief, mut sent, directory) = fixture_with_history(history).await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		chief.observe_async_question_item("opaque thread/1","opaque turn/1",&json!({"id":"question","type":"agentMessage","delivery":"async","questions":[{"title":"Which?"}]})).await.unwrap();
		while sent.try_recv().is_ok() {}
		let id = decodex_protocol::chief_async_question_id("question", 0);
		assert!(chief.answer_async_question("chief", &id, "A", "once").await.is_err());
		assert_eq!(chief.store.read_chief_async_questions("chief".into()).await.unwrap().len(), 1);
		chief.wake_pending().await.unwrap();
		let mut count = 0;
		while let Ok(request) = sent.try_recv() {
			assert_eq!(request["method"], "turn/steer");
			count += 1;
		}
		assert_eq!(count, 1);
		assert!(chief.store.list_undelivered_chief_events(100).await.unwrap().is_empty());
		assert_eq!(
			chief.store.chief_async_answer_pending("chief".into(), id.clone()).await.unwrap(),
			uncertain
		);
		if uncertain {
			let root = decodex_core::DecodexRoot::new(
				directory.path().canonicalize().unwrap().join("root"),
			)
			.unwrap();
			let (mut reopened, mut reopened_sent, _other) = fixture().await;
			reopened.store = SqliteStore::open(&root.paths()).unwrap();
			assert!(
				reopened
					.answer_async_question("chief", &id, "A", "new-command-after-restart")
					.await
					.is_err()
			);
			assert!(reopened_sent.try_recv().is_err());
		}
	}
}

#[tokio::test]
async fn other_client_prompt_sync_requires_exact_item_and_replay_preserves_newer_questions() {
	for contains_prompt in [false, true] {
		let question = |id: &str| json!({"id":id,"type":"agentMessage","delivery":"async","questions":[{"title":"Question"}]});
		let prompt = json!({"id":"remote-prompt","type":"userMessage","content":[{"type":"text","text":"Move on"}]});
		let mut items = vec![question("old")];
		if contains_prompt {
			items.push(prompt.clone());
		}
		items.push(question("new"));
		let history = json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"opaque turn/1","status":"inProgress","items":items}]}}});
		let (mut chief, mut sent, _directory) = fixture_with_history(history).await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		while sent.try_recv().is_ok() {}
		for _ in 0..2 {
			chief
				.observe_notification(
					"item/completed",
					&json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","item":prompt}),
				)
				.await
				.unwrap();
			assert_eq!(
				chief.store.chief_async_questions_recovering("chief".into()).await.unwrap(),
				!contains_prompt
			);
			let questions = chief.store.read_chief_async_questions("chief".into()).await.unwrap();
			if contains_prompt {
				assert_eq!(questions.len(), 1);
				assert_eq!(questions[0].item_id, "new");
			} else {
				assert!(questions.is_empty());
			}
		}
		while let Ok(request) = sent.try_recv() {
			assert_eq!(request["method"], "thread/read");
		}
	}
}

#[tokio::test]
async fn async_answer_does_not_fork_an_old_manager_thread_for_tool_upgrade() {
	let (mut chief, mut sent, directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	chief.observe_async_question_item("opaque thread/1","opaque turn/1",&json!({"id":"question","type":"agentMessage","delivery":"async","questions":[{"title":"Which?"}]})).await.unwrap();
	chief.store.complete_chief_turn("chief".into(), "opaque turn/1".into()).await.unwrap();
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let db = rusqlite::Connection::open(root.paths().product_database_file()).unwrap();
	db.execute("DELETE FROM chief_tool_versions WHERE work_id='chief'", []).unwrap();
	drop(db);
	while sent.try_recv().is_ok() {}
	chief
		.answer_async_question(
			"chief",
			&decodex_protocol::chief_async_question_id("question", 0),
			"A",
			"answer-old-manager",
		)
		.await
		.unwrap();
	let mut started = false;
	while let Ok(request) = sent.try_recv() {
		assert!(
			["thread/resume", "thread/inject_items", "turn/start"]
				.contains(&request["method"].as_str().unwrap())
		);
		assert_eq!(request["params"]["threadId"], "opaque thread/1");
		started |= request["method"] == "turn/start";
	}
	assert!(started);
	assert_eq!(chief.store.chief_tool_version("chief".into()).await.unwrap(), 1);
}

#[tokio::test]
async fn misalignment_precaution_survives_reopen_and_blocks_ordinary_dispatch() {
	let (mut chief, mut sent, directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	chief.enqueue_user_message("chief", "queued-before-stop", "Queued work").await.unwrap();
	let error = json!({"codexErrorInfo":"misalignmentPolicyViolation","misalignment":{"detailedExplanation":"Review the scope.","steer":{"message":"Continue with the clarified scope"}}});
	chief
		.observe_notification(
			"error",
			&json!({"threadId":"opaque thread/1","turnId":"stale","willRetry":false,"error":error}),
		)
		.await
		.unwrap();
	assert!(chief.store.chief_misalignment("chief".into()).await.unwrap().is_none());
	chief
		.observe_notification(
			"error",
			&json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","willRetry":true,"error":error}),
		)
		.await
		.unwrap();
	assert!(chief.store.chief_misalignment("chief".into()).await.unwrap().is_none());
	chief
		.observe_notification(
			"error",
			&json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","willRetry":false,"error":error}),
		)
		.await
		.unwrap();
	chief
		.observe_misalignment(
			"opaque thread/1",
			"opaque turn/1",
			&json!({"codexErrorInfo":"misalignmentPolicyViolation"}),
		)
		.await
		.unwrap();
	let saved = chief.store.chief_misalignment("chief".into()).await.unwrap().unwrap();
	assert!(chief.enqueue_user_message("chief", "after-stop", "New work").await.is_err());
	assert!(chief.store.list_undelivered_chief_events(100).await.unwrap().is_empty());

	assert!(saved.details_json.as_deref().unwrap().contains("Review the scope."));
	assert!(chief.steer_work("chief", "opaque turn/1", "ordinary", "Continue", &[]).await.is_err());
	chief.store.complete_chief_turn("chief".into(), "opaque turn/1".into()).await.unwrap();
	assert!(chief.continue_worker("chief", "Continue").await.is_err());
	assert!(sent.try_recv().is_err());
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let reopened = SqliteStore::open(&root.paths()).unwrap();
	assert_eq!(reopened.chief_misalignment("chief".into()).await.unwrap(), Some(saved));
}

fn live_review_token(
	chief: &ChiefCoordinator,
	review: &decodex_database::ChiefMisalignment,
) -> String {
	let (_, guard) =
		chief.client.live_misalignment_review(&review.thread_id, &review.turn_id).unwrap();
	super::misalignment::review_token(review, &guard).unwrap()
}

#[tokio::test]
async fn explicit_misalignment_continuation_uses_native_override_and_clears_after_ack() {
	let error = json!({"codexErrorInfo":"misalignmentPolicyViolation","misalignment":{"detailedExplanation":"Review scope","steer":{"message":"Clarified scope"}}});
	let history = json!({"_live_misalignment":error,"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"opaque turn/1","status":"failed","error":{"codexErrorInfo":"misalignmentPolicyViolation"},"items":[]}]}}});
	let (mut chief, mut sent, _directory) = fixture_with_history(history).await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	chief.observe_misalignment("opaque thread/1", "opaque turn/1", &error).await.unwrap();
	chief.store.complete_chief_turn("chief".into(), "opaque turn/1".into()).await.unwrap();
	let review = chief.store.chief_misalignment("chief".into()).await.unwrap().unwrap();
	while sent.try_recv().is_ok() {}
	let token = live_review_token(&chief, &review);
	assert!(
		chief
			.continue_misalignment("chief", review.clone(), "stale-click", "older-live-evidence")
			.await
			.is_err()
	);
	assert!(sent.try_recv().is_err());
	chief.continue_misalignment("chief", review, "acknowledged", &token).await.unwrap();
	let mut turns = Vec::new();
	while let Ok(request) = sent.try_recv() {
		if request["method"] == "turn/start" {
			turns.push(request);
		}
	}
	assert_eq!(turns.len(), 1);
	assert_eq!(turns[0]["params"]["threadId"], "opaque thread/1");
	assert_eq!(turns[0]["params"]["input"][0]["text"], "Clarified scope");
	let metadata: Value = serde_json::from_str(
		turns[0]["params"]["responsesapiClientMetadata"]["misalignment_override"].as_str().unwrap(),
	)
	.unwrap();
	assert!(metadata["timestamp"].as_u64().unwrap() > 0);
	assert!(chief.store.chief_misalignment("chief".into()).await.unwrap().is_none());
	assert_eq!(
		chief.store.get_chief_work_item("chief".into()).await.unwrap().active_turn_id.as_deref(),
		Some("opaque turn/2")
	);
}

#[tokio::test]
async fn misalignment_stale_rejected_and_uncertain_continuations_keep_precaution() {
	for outcome in ["changed", "rejected", "uncertain", "reverted"] {
		let error = json!({"codexErrorInfo":"misalignmentPolicyViolation","misalignment":{"detailedExplanation":"Review scope","steer":{"message":"Clarified scope"}}});
		let mut native_error = error.clone();
		if outcome == "changed" {
			native_error["misalignment"]["detailedExplanation"] = json!("New findings");
		}
		let history = json!({"_live_misalignment":error,"_misalignment_revert_on_read":outcome=="reverted","_continuation_disconnect":outcome=="uncertain","_continuation_reject":outcome=="rejected","opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"opaque turn/1","status":"failed","error":native_error,"items":[]}]}}});
		let (mut chief, mut sent, directory) = fixture_with_history(history).await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		chief.observe_misalignment("opaque thread/1", "opaque turn/1", &error).await.unwrap();
		chief.store.complete_chief_turn("chief".into(), "opaque turn/1".into()).await.unwrap();
		let review = chief.store.chief_misalignment("chief".into()).await.unwrap().unwrap();
		while sent.try_recv().is_ok() {}
		let token = live_review_token(&chief, &review);
		let failure = chief
			.continue_misalignment("chief", review.clone(), "acknowledged", &token)
			.await
			.unwrap_err();
		assert_eq!(matches!(failure, ChiefError::Rejected(_)), outcome != "uncertain");
		assert!(chief.store.chief_misalignment("chief".into()).await.unwrap().is_some());
		let state = chief.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state;
		assert_eq!(
			state,
			if outcome == "uncertain" {
				decodex_database::ChiefDispatchState::Dispatching
			} else {
				decodex_database::ChiefDispatchState::Idle
			}
		);
		let mut starts = 0;
		while let Ok(request) = sent.try_recv() {
			starts += usize::from(request["method"] == "turn/start");
		}
		assert_eq!(starts, usize::from(!["changed", "reverted"].contains(&outcome)));
		if outcome == "uncertain" {
			let root = decodex_core::DecodexRoot::new(
				directory.path().canonicalize().unwrap().join("root"),
			)
			.unwrap();
			let (mut reopened, mut requests, _other) = fixture().await;
			reopened.store = SqliteStore::open(&root.paths()).unwrap();
			assert!(
				reopened.continue_misalignment("chief", review, "new-key", &token).await.is_err()
			);
			assert!(requests.try_recv().is_err());
		}
	}
}

#[tokio::test]
async fn misalignment_saved_details_cannot_authorize_a_reconnected_transport() {
	let error = json!({"codexErrorInfo":"misalignmentPolicyViolation","misalignment":{"detailedExplanation":"Review scope","steer":{"message":"Clarified scope"}}});
	let (mut chief, _sent, directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	chief.observe_misalignment("opaque thread/1", "opaque turn/1", &error).await.unwrap();
	chief.store.complete_chief_turn("chief".into(), "opaque turn/1".into()).await.unwrap();
	let review = chief.store.chief_misalignment("chief".into()).await.unwrap().unwrap();
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	drop(chief);
	let (mut reopened, mut requests, _other) = fixture().await;
	reopened.store = SqliteStore::open(&root.paths()).unwrap();
	assert!(matches!(
		reopened
			.continue_misalignment("chief", review.clone(), "confirm", "old-source-review")
			.await,
		Err(ChiefError::Rejected(_))
	));
	assert!(requests.try_recv().is_err());
	assert_eq!(reopened.store.chief_misalignment("chief".into()).await.unwrap(), Some(review));
	assert_eq!(
		reopened.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		decodex_database::ChiefDispatchState::Idle
	);
}

#[tokio::test]
async fn idle_thread_recovery_restores_only_latest_misalignment_failure() {
	for stopped in [false, true] {
		let error = json!({"codexErrorInfo":"misalignmentPolicyViolation","misalignment":{"detailedExplanation":"Recovered findings","steer":{"message":"Clarified scope"}}});
		let history = json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"older","status":"failed","error":error,"items":[]},{"id":"latest","status":if stopped {"failed"} else {"completed"},"error":if stopped {error.clone()} else {Value::Null},"items":[]}]}}});
		let (mut chief, mut sent, _directory) = fixture_with_history(history).await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		chief.store.complete_chief_turn("chief".into(), "opaque turn/1".into()).await.unwrap();
		while sent.try_recv().is_ok() {}
		chief.recover_persisted().await.unwrap();
		let precaution = chief.store.chief_misalignment("chief".into()).await.unwrap();
		assert_eq!(precaution.is_some(), stopped);
		if let Some(precaution) = precaution {
			assert_eq!(precaution.turn_id, "latest");
			assert!(precaution.details_json.unwrap().contains("Recovered findings"));
		}
		while let Ok(request) = sent.try_recv() {
			assert_eq!(request["method"], "thread/read");
		}
	}
}

#[tokio::test]
async fn misalignment_does_not_send_or_consume_pending_provider_approval() {
	let (mut chief, mut sent, _directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	let id = RequestId::Number(7);
	chief.handle_event(ServerEvent::Request {id:id.clone(),method:"item/commandExecution/requestApproval".into(),params:json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","itemId":"item","command":"pwd"})}).await.unwrap();
	let event = chief.pending_requests[&id];
	chief
		.observe_misalignment(
			"opaque thread/1",
			"opaque turn/1",
			&json!({"codexErrorInfo":"misalignmentPolicyViolation"}),
		)
		.await
		.unwrap();
	while sent.try_recv().is_ok() {}
	assert!(chief.respond_pending_event(event, json!({"decision":"accept"})).await.is_err());
	assert_eq!(chief.pending_requests[&id], event);
	assert!(chief.store.get_chief_inbox_event(event).await.unwrap().disposition.is_none());
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn mcp_form_response_validates_original_schema_before_consuming_live_request() {
	for mode in ["form", "openai/form", "openaiForm"] {
		let (mut chief, mut sent, _directory) = fixture().await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		chief.store.complete_chief_turn("chief".into(), "opaque turn/1".into()).await.unwrap();
		let id = RequestId::String("mcp-form".into());
		chief.handle_event(ServerEvent::Request {id:id.clone(),method:"mcpServer/elicitation/request".into(),params:json!({"threadId":"opaque thread/1","turnId":null,"serverName":"test","mode":mode,"requestedSchema":{"type":"object","properties":{"allow":{"type":"boolean"}},"required":["allow"]}})}).await.unwrap();
		let event = chief.pending_requests[&id];
		while sent.try_recv().is_ok() {}
		for response in [
			json!({"action":"accept","content":{"allow":"true"}}),
			json!({"action":"accept","content":{"allow":true},"_meta":{"persist":"always"}}),
			json!({"decision":"accept"}),
		] {
			assert!(matches!(
				chief.respond_pending_event(event, response).await,
				Err(ChiefError::Rejected(_))
			));
			assert_eq!(chief.pending_requests[&id], event);
			assert!(sent.try_recv().is_err());
		}
		chief
			.respond_pending_event(
				event,
				json!({"action":"accept","content":{"allow":false},"_meta":null}),
			)
			.await
			.unwrap();
		let reply = sent.recv().await.unwrap();
		assert_eq!(reply["id"], "mcp-form");
		assert_eq!(reply["result"]["content"]["allow"], false);
		assert!(!chief.pending_requests.contains_key(&id));
		assert!(
			chief
				.respond_pending_event(event, json!({"action":"cancel","content":null}))
				.await
				.is_err()
		);
		assert!(sent.try_recv().is_err());
	}
}

#[tokio::test]
async fn standalone_mcp_resolution_and_reconnection_never_replay_a_reply() {
	for mode in ["form", "openai/form", "openaiForm"] {
		let (mut chief, mut sent, _directory) = fixture().await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		chief.store.complete_chief_turn("chief".into(), "opaque turn/1".into()).await.unwrap();
		let id = RequestId::Number(17);
		let params = json!({"threadId":"opaque thread/1","turnId":null,"serverName":"test","mode":mode,"requestedSchema":null});
		chief
			.handle_event(ServerEvent::Request {
				id: id.clone(),
				method: "mcpServer/elicitation/request".into(),
				params: params.clone(),
			})
			.await
			.unwrap();
		let old_event = chief.pending_requests[&id];
		let mut reconnected =
			ChiefCoordinator::new(chief.store.clone(), chief.client.clone(), chief.config.clone())
				.unwrap();
		while sent.try_recv().is_ok() {}
		assert!(
			reconnected
				.respond_pending_event(old_event, json!({"action":"accept","content":null}))
				.await
				.is_err()
		);
		assert!(sent.try_recv().is_err());
		reconnected
			.handle_event(ServerEvent::Request {
				id: id.clone(),
				method: "mcpServer/elicitation/request".into(),
				params,
			})
			.await
			.unwrap();
		let event = reconnected.pending_requests[&id];
		assert_ne!(event, old_event);
		reconnected
			.handle_event(ServerEvent::Notification {
				method: "serverRequest/resolved".into(),
				params: json!({"threadId":"wrong-thread","requestId":17}),
			})
			.await
			.unwrap();
		assert_eq!(reconnected.pending_requests[&id], event);
		reconnected
			.handle_event(ServerEvent::Notification {
				method: "serverRequest/resolved".into(),
				params: json!({"threadId":"opaque thread/1","requestId":17}),
			})
			.await
			.unwrap();
		assert!(!reconnected.pending_requests.contains_key(&id));
		assert_eq!(
			reconnected.store.get_chief_inbox_event(event).await.unwrap().disposition,
			Some(ChiefDisposition::Resolved)
		);
		assert!(
			reconnected
				.respond_pending_event(event, json!({"action":"accept","content":null}))
				.await
				.is_err()
		);
		assert!(sent.try_recv().is_err());
	}
}
