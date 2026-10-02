#[path = "tests/archive.rs"] mod archive;
#[path = "tests/async_recovery.rs"] mod async_recovery;
#[path = "tests/auth_recovery.rs"] mod auth_recovery;
#[path = "tests/background_terminals.rs"] mod background_terminals;
#[path = "tests/capacity.rs"] mod capacity;
#[path = "tests/closing_resume.rs"] mod closing_resume;
#[path = "tests/drain_rejection.rs"] mod drain_rejection;
#[path = "tests/external_context.rs"] mod external_context;
#[path = "tests/guardian.rs"] mod guardian;
#[path = "tests/inbox_carryover.rs"] mod inbox_carryover;
#[path = "tests/large_approval.rs"] mod large_approval;
#[path = "tests/live_file_approval.rs"] mod live_file_approval;
#[path = "tests/misalignment_recovery.rs"] mod misalignment_recovery;
#[path = "tests/native_checklist.rs"] mod native_checklist;
#[path = "tests/native_goal_fixture.rs"] mod native_goal_fixture;
#[path = "tests/native_goal_recovery.rs"] mod native_goal_recovery;
#[path = "tests/native_goals.rs"] mod native_goals;
#[path = "tests/native_mcp_forms.rs"] mod native_mcp_forms;
#[path = "tests/native_misalignment.rs"] mod native_misalignment;
#[path = "tests/native_partial_output.rs"] mod native_partial_output;
#[path = "tests/native_permissions.rs"] mod native_permissions;
#[path = "tests/native_plan.rs"] mod native_plan;
#[path = "tests/native_settings.rs"] mod native_settings;
#[path = "tests/native_subagent_live.rs"] mod native_subagent_live;
#[path = "tests/native_subagents.rs"] mod native_subagents;
#[path = "tests/native_task_references.rs"] mod native_task_references;
#[path = "tests/prompt_edit.rs"] mod prompt_edit;
#[path = "tests/reasoning_summary.rs"] mod reasoning_summary;
#[path = "tests/result_integrity.rs"] mod result_integrity;
#[path = "tests/settings_observations.rs"] mod settings_observations;
#[path = "tests/steer_receipts.rs"] mod steer_receipts;
#[path = "tests/task_history.rs"] mod task_history;
#[path = "tests/unsent_input.rs"] mod unsent_input;

use std::{iter, slice, time::Duration};

use rusqlite::Connection;
use tempfile::TempDir;
use tokio::{
	io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, WriteHalf},
	sync::{
		mpsc,
		mpsc::{UnboundedReceiver, UnboundedSender},
	},
	task, time,
};

use crate::{
	agent::{
		self, AgentConfig, AgentCoordinator, AgentDisposition, AgentError, AgentInboxEvent,
		AgentInputExtras, AgentWorkItem, AgentWorkStatus, AppServerClient, ClientError,
		EnqueueAgentEvent, MAX_WAKE_BATCH_BYTES, RequestId, ServerEvent, SqliteStore, Value,
		async_projection::Projection, misalignment, result_messages, timeline, timeline::metrics,
	},
	application,
};
use decodex_core::DecodexRoot;
use decodex_database::{AgentMisalignment, AgentOutputUpdate};
use decodex_protocol::{
	AgentActivityDto, AgentAsyncQuestionDto, AgentRequestedDecision, AgentTimelineContent,
};

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
		writer: &mut WriteHalf<DuplexStream>,
	) -> Option<bool> {
		if request["method"] == "turn/start"
			&& (history["_turn_draining"] == true
				|| (history["_capacity_draining"] == true
					&& request["params"]["toolOutput"]["name"] == "capacity_retry")
				|| (self.injected && history["_turn_draining_after_injection"] == true))
		{
			writer.write_all(format!("{}\n",serde_json::json!({"id":request["id"],"error":{"code":-32_600,"message":history["_refusal_message"].as_str().unwrap_or("Server is draining; retry after reconnecting")}})).as_bytes()).await.unwrap();

			return Some(true);
		}
		if request["method"] == "thread/resume" && self.resume_failures > 0 {
			self.resume_failures -= 1;

			let message = if history["_resume_closing"] == true {
				format!(
					"thread {} is closing; retry thread/resume after the thread is closed",
					request["params"]["threadId"].as_str().unwrap()
				)
			} else {
				"thread private-id already has an active writer".into()
			};
			let error = history
				.get("_resume_error")
				.cloned()
				.unwrap_or_else(|| serde_json::json!({"code":-32_600,"message":message}));
			let mut frame = serde_json::json!({"id":request["id"],"error":error}).to_string();

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
							serde_json::json!({"id":request["id"],"error":{"code":-32_600,"message":"restore rejected"}})
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
					serde_json::json!({"id":request["id"],"error":{"code":-32_600,"message":"approval rejected"}})
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
				serde_json::json!({"id":request["id"],"error":{"code":-32_600,"message":"turn ended"}})
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
					serde_json::json!({"id":request["id"],"error":{"code":-32_600,"message":"continuation rejected private-steer-sentinel"}})
				);

				writer.write_all(frame.as_bytes()).await.unwrap();

				return Some(true);
			}
		}

		None
	}
}

pub(super) async fn fixture_with_history(
	history: Value,
) -> (AgentCoordinator, UnboundedReceiver<Value>, TempDir) {
	let directory = tempfile::tempdir().unwrap();
	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();

	root.paths().ensure_layout().unwrap();

	let store = SqliteStore::open(&root.paths()).unwrap();
	let (client_io, server_io) = io::duplex(65_536);
	let (reader, writer) = io::split(client_io);
	let (client, mut events) = AppServerClient::from_io(reader, writer);

	tokio::spawn(async move { while events.recv().await.is_some() {} });

	let (sent, received) = mpsc::unbounded_channel();

	tokio::spawn(serve_fixture(server_io, history, sent));

	(
		AgentCoordinator::new(
			store,
			client,
			AgentConfig::new(
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

fn fixture_thread_read(
	request: &Value,
	history: &Value,
	settings: &std::collections::HashMap<String, Value>,
	started_turns: u64,
) -> Value {
	let id = request["params"]["threadId"].as_str().unwrap();
	let mut result = history.get(id).cloned().unwrap_or_else(
		|| serde_json::json!({"thread":{"id":id,"turns":[],"status":{"type":"idle"}}}),
	);
	let configured = settings.get(id).cloned().unwrap_or_else(
		|| serde_json::json!({"model":"selected-model","reasoningEffort":"high","modelProvider":"openai"}),
	);

	for field in ["model", "reasoningEffort", "modelProvider"] {
		if result["thread"].get(field).is_none() {
			result["thread"][field] = configured[field].clone();
		}
	}

	if result["thread"]["cwd"].is_null() {
		result["thread"]["cwd"] = serde_json::json!("/tmp");
	}
	if history["_started_turns_only"] == true
		&& let Some(turns) = result["thread"]["turns"].as_array_mut()
	{
		turns.truncate(usize::try_from(started_turns).expect("fixture turn count"));
	}
	if result["thread"]["historyMode"] == "paginated" {
		assert_ne!(request["params"]["includeTurns"], true);

		result["thread"]["turns"] = serde_json::json!([]);
	}

	result
}

#[test]
fn partial_message_settings_preserve_existing_values_and_explicit_standard_clears_tier() {
	let baseline = serde_json::json!({"input":[],"model":"native-model","effort":"high","serviceTier":"priority"});
	let mut params = baseline.clone();
	let message = |execution: Value| {
		serde_json::json!({"options":{"execution":execution,"attachments":[]}}).to_string()
	};

	agent::apply_message_options(&mut params, &message(serde_json::json!({}))).unwrap();

	assert_eq!(params, baseline);

	agent::apply_message_options(
		&mut params,
		&message(serde_json::json!({"reasoning_effort":"medium"})),
	)
	.unwrap();

	assert_eq!(params["model"], "native-model");
	assert_eq!(params["effort"], "medium");
	assert_eq!(params["serviceTier"], "priority");
	assert!(params.get("serviceTierForTurn").is_none());

	agent::apply_message_options(
		&mut params,
		&message(serde_json::json!({"service_tier":"default"})),
	)
	.unwrap();

	assert!(params["serviceTier"].is_null());
	assert_eq!(params["serviceTierForTurn"], "default");
	assert_eq!(params["model"], "native-model");
	assert_eq!(params["effort"], "medium");
}

#[test]
fn steering_receipt_preserves_turn_settings_when_carried_as_evidence() {
	let mut params = serde_json::json!({"input":[],"model":"current-model","effort":"high","serviceTier":"priority"});

	agent::apply_message_options(&mut params,&serde_json::json!({"text":"Supplement","options":{"attachments":[{"path":"/tmp/steer.png","image":true}]}}).to_string()).unwrap();

	assert_eq!(params["model"], "current-model");
	assert_eq!(params["effort"], "high");
	assert_eq!(params["serviceTier"], "priority");
	assert_eq!(params["input"][0]["type"], "localImage");
}

fn live_review_token(agent: &AgentCoordinator, review: &AgentMisalignment) -> String {
	let (_, guard) =
		agent.client.live_misalignment_review(&review.thread_id, &review.turn_id).unwrap();

	misalignment::review_token(review, &guard).unwrap()
}

#[tokio::test]
async fn native_effort_inheritance_omits_overrides_and_preserves_legacy_config() {
	let (mut agent, mut sent, _directory) = fixture().await;
	let legacy = serde_json::to_value(&agent.config).unwrap();

	assert!(legacy["agent_effort"].is_string() && legacy["worker_effort"].is_string());

	let restored: AgentConfig = serde_json::from_value(legacy).unwrap();

	assert_eq!(restored.agent_effort, agent.config.agent_effort);

	agent.config.agent_effort = None;
	agent.config.worker_effort = None;

	for manager in [false, true] {
		assert!(agent.thread_params(manager)["config"].get("model_reasoning_effort").is_none());
	}

	let saved = serde_json::to_vec(&agent.config).unwrap();

	assert!(serde_json::from_slice::<AgentConfig>(&saved).unwrap().agent_effort.is_none());

	agent.start_agent("agent", "Coordinate").await.unwrap();

	let mut starts = 0;

	while let Ok(request) = sent.try_recv() {
		match request["method"].as_str() {
			Some("thread/start") => {
				assert!(request["params"]["config"].get("model_reasoning_effort").is_none());

				starts += 1;
			},
			Some("turn/start") => {
				assert!(request["params"].get("effort").is_none());

				starts += 1;
			},
			_ => {},
		}
	}

	assert_eq!(starts, 2);

	agent.config.agent_effort = Some("none".into());

	assert_eq!(agent.thread_params(true)["config"]["model_reasoning_effort"], "none");
}

#[tokio::test]
async fn advertised_efforts_survive_coordinator_admission_and_dispatch() {
	for effort in ["persistent", "future-provider-reasoning-effort-over-32-bytes"] {
		let (fixture, mut sent, _directory) = fixture().await;
		let mut config = fixture.config.clone();

		config.agent_effort = Some(effort.into());
		config.worker_effort = Some(effort.into());

		let mut agent =
			AgentCoordinator::new(fixture.store.clone(), fixture.client.clone(), config.clone())
				.unwrap();

		agent.start_agent("agent", "Coordinate").await.unwrap();

		let mut starts = 0;

		while let Ok(request) = sent.try_recv() {
			match request["method"].as_str() {
				Some("thread/start") => {
					assert_eq!(request["params"]["config"]["model_reasoning_effort"], effort);

					starts += 1;
				},
				Some("turn/start") => {
					assert_eq!(request["params"]["effort"], effort);

					starts += 1;
				},
				_ => {},
			}
		}

		assert_eq!(starts, 2);

		for field in [&mut config.agent_effort, &mut config.worker_effort] {
			*field = Some("invalid\neffort".into());
		}

		assert!(AgentCoordinator::new(fixture.store, fixture.client, config).is_err());
	}
}

#[tokio::test]
async fn subagent_activity_survives_parent_completion_and_restart_without_waking_work() {
	let (mut agent, mut sent, directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	let (io, mut write) = io::duplex(8_192);
	let (read, writer) = io::split(io);
	let (_client, mut events) = AppServerClient::from_io(read, writer);

	for (index, kind) in ["started", "interacted", "interrupted", "completed"].iter().enumerate() {
		if *kind == "completed" {
			agent.handle_event(ServerEvent::Notification { method:"turn/completed".into(), params:serde_json::json!({"threadId":"opaque thread/1","turn":{"id":"opaque turn/1","status":"completed","items":[]}})}).await.unwrap();
		}

		for (thread, turn) in [
			("foreign", "opaque turn/1"),
			("opaque thread/1", "unknown"),
			("opaque thread/1", "opaque turn/1"),
		] {
			for method in ["item/started", "item/completed", "item/completed"] {
				let wire = serde_json::json!({"method":method,"params":{"threadId":thread,"turnId":turn,"item":{"id":format!("activity-{index}"),"type":"subAgentActivity","kind":kind,"agentThreadId":"child-thread","agentPath":"/root/worker","prompt":"PRIVATE_PROMPT"}}});

				write.write_all(format!("{wire}\n").as_bytes()).await.unwrap();

				let event =
					time::timeout(Duration::from_secs(2), events.recv()).await.unwrap().unwrap();

				agent.handle_event(event).await.unwrap();
			}
		}
	}

	while sent.try_recv().is_ok() {}

	agent.wake_pending().await.unwrap();

	assert!(sent.try_recv().is_err());

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();

	drop(agent);

	let store = SqliteStore::open(&root.paths()).unwrap();
	let (history, _) = store.read_agent_transcript("agent".into(), None, 32).await.unwrap();
	let activities: Vec<AgentActivityDto> = history
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
	assert!(store.list_agent_wake_events("agent".into(), 32).await.unwrap().is_empty());

	let work = store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Idle);
	assert_eq!(work.status, decodex_database::AgentWorkStatus::Open);
}

#[tokio::test]
async fn native_activity_duration_pairs_exact_receipts_and_survives_restart() {
	let (mut agent, _sent, directory) = fixture().await;

	agent.start_agent("agent", "Work").await.unwrap();

	let cases = [
		("paired", Some(1_000), Some(2_250), None, Some(1_250)),
		("explicit", Some(1_000), Some(2_250), Some(900), Some(900)),
		("zero", Some(1_000), Some(1_000), None, Some(0)),
		("reversed", Some(2_250), Some(1_000), None, None),
		("missing-start", None, Some(2_250), None, None),
		("missing-end", Some(1_000), None, None, None),
		("negative", Some(-1), Some(2_250), None, None),
	];

	for (id, start, end, explicit, _) in cases {
		// A receipt from another thread must never provide the missing start.
		for (thread, stamp) in [("foreign", Some(500)), ("opaque thread/1", start)] {
			agent
				.handle_event(ServerEvent::Notification {
					method: "item/started".into(),
					params: serde_json::json!({"threadId":thread,"turnId":"opaque turn/1","startedAtMs":stamp,
					"item":{"id":id,"type":"webSearch"}}),
				})
				.await
				.unwrap();
		}
		for _ in 0..2 {
			agent
				.handle_event(ServerEvent::Notification {
					method: "item/completed".into(),
					params: serde_json::json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","completedAtMs":end,
					"item":{"id":id,"type":"webSearch","durationMs":explicit}}),
				})
				.await
				.unwrap();
		}
	}

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();

	drop(agent);

	let store = SqliteStore::open(&root.paths()).unwrap();
	let (history, _) = store.read_agent_transcript("agent".into(), None, 32).await.unwrap();
	let activities: Vec<AgentActivityDto> = history
		.iter()
		.filter(|event| event.event_kind == "activity_completed")
		.map(|event| serde_json::from_str(&event.payload).unwrap())
		.collect();

	assert_eq!(activities.len(), cases.len());

	let rows = cases
		.iter()
		.enumerate()
		.map(|(position, (id, _, _, explicit, _))| {
			serde_json::json!({"type":"item","position":position,"turnId":"opaque turn/1",
			"item":{"id":id,"type":"webSearch","status":"completed","durationMs":explicit}})
		})
		.collect::<Vec<_>>();
	let native =
		serde_json::json!({"data":rows,"nextCursor":null,"activeRealtimeSessionAtPageStart":null});
	let mut page = timeline::project("opaque thread/1", &native).unwrap();

	metrics::enrich(&store, "agent", &mut page).await.unwrap();

	for (id, _, _, _, expected) in cases {
		assert_eq!(
			activities.iter().find(|activity| activity.item_id == id).unwrap().duration_ms,
			expected,
			"{id}"
		);

		let activity = page
			.entries
			.iter()
			.find_map(|entry| match &entry.content {
				AgentTimelineContent::Item { item_id, activity, .. } if item_id == id =>
					activity.as_ref(),
				_ => None,
			})
			.unwrap();

		assert_eq!(activity.duration_ms, expected, "native timeline: {id}");
	}

	let mut other = timeline::project("foreign", &native).unwrap();

	metrics::enrich(&store, "agent", &mut other).await.unwrap();

	assert!(
		matches!(&other.entries[0].content, AgentTimelineContent::Item{ activity: Some(activity), .. } if activity.duration_ms.is_none())
	);
	assert!(store.list_agent_wake_events("agent".into(), 32).await.unwrap().is_empty());
}

#[tokio::test]
async fn terminal_readback_recovers_missed_subagent_activity() {
	let history = serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","status":{"type":"idle"},"turns":[{"id":"opaque turn/1","status":"completed","items":[{"id":"recovered","type":"subAgentActivity","kind":"started","agentThreadId":"child","agentPath":"/root/worker"}]}]}}});
	let (mut agent, _sent, _directory) = fixture_with_history(history).await;

	agent.start_agent("agent", "Coordinate").await.unwrap();
	agent.recover_persisted().await.unwrap();

	let (history, _) = agent.store.read_agent_transcript("agent".into(), None, 32).await.unwrap();
	let events: Vec<_> = history.iter().filter(|e| e.event_kind == "activity_completed").collect();

	assert_eq!(events.len(), 1);
	assert!(events[0].payload.contains("Subagent started"));
}

#[tokio::test]
async fn strict_review_notice_is_turn_bound_and_does_not_wake_or_stop_execution() {
	let (mut coordinator, mut sent, _directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let (read, mut write) = io::duplex(8_192);
	let (reader, writer) = io::split(read);
	let (_client, mut events) = AppServerClient::from_io(reader, writer);

	for (thread, turn, started) in [
		("wrong", "opaque turn/1", serde_json::json!(1)),
		("opaque thread/1", "old", serde_json::json!(1)),
		("opaque thread/1", "opaque turn/1", serde_json::json!(-1)),
		("opaque thread/1", "opaque turn/1", serde_json::json!("1")),
		("opaque thread/1", "opaque turn/1", serde_json::json!(1)),
		("opaque thread/1", "opaque turn/1", serde_json::json!(2)),
	] {
		let wire = serde_json::json!({"method":"autoApprovalReview/strictReviewRequired","params":{"threadId":thread,"turnId":turn,"startedAtMs":started}});

		write.write_all(format!("{wire}\n").as_bytes()).await.unwrap();

		let event = time::timeout(Duration::from_secs(2), events.recv()).await.unwrap().unwrap();

		coordinator.handle_event(event).await.unwrap();
	}

	let (history, _) =
		coordinator.store.read_agent_transcript("agent".into(), None, 32).await.unwrap();

	assert_eq!(
		history.iter().filter(|event| event.event_kind == "strict_review_notice").count(),
		1
	);
	assert!(coordinator.store.list_pending_agent_events(32).await.unwrap().is_empty());
	assert!(coordinator.store.list_agent_wake_events("agent".into(), 32).await.unwrap().is_empty());
	assert_eq!(
		coordinator.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Running
	);

	coordinator.wake_pending().await.unwrap();

	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn asynchronous_questions_and_usage_are_observed_without_completing_or_waking_work() {
	let (mut coordinator, mut sent, _directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let message = serde_json::json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","item":{
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

	// Freeform async updates use final_answer without completing the active turn.
	let update = serde_json::json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","item":{
		"id":"freeform-update","type":"agentMessage","delivery":"async",
		"phase":"final_answer","text":"Please review this finding while I continue."}});

	for _ in 0..2 {
		coordinator
			.handle_event(ServerEvent::Notification {
				method: "item/completed".into(),
				params: update.clone(),
			})
			.await
			.unwrap();
	}

	assert_eq!(
		coordinator.store.read_agent_async_questions("agent".into()).await.unwrap().len(),
		1
	);

	let counts = serde_json::json!({"totalTokens":1_200,"inputTokens":1_000,"cachedInputTokens":500,"outputTokens":200,"reasoningOutputTokens":100});

	coordinator.handle_event(ServerEvent::Notification { method:"thread/tokenUsage/updated".into(), params:serde_json::json!({
		"threadId":"opaque thread/1","turnId":"opaque turn/1","tokenUsage":{"total":counts,"last":counts,"modelContextWindow":128_000}
	}) }).await.unwrap();
	coordinator.handle_event(ServerEvent::Notification { method:"item/completed".into(), params:serde_json::json!({
		"threadId":"opaque thread/1","turnId":"opaque turn/1","item":{"type":"contextCompaction","id":"compact"}
	}) }).await.unwrap();

	let work = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Running);

	let history = coordinator.store.read_agent_work_events("agent".into(), 10).await.unwrap();

	assert_eq!(history.iter().filter(|event| event.event_kind == "assistant_message").count(), 2);
	assert_eq!(history.iter().filter(|event| event.event_kind == "context_compacted").count(), 1);
	assert_eq!(history.iter().filter(|event| event.event_kind == "activity_completed").count(), 1);
	assert!(coordinator.store.read_agent_usage("agent".into()).await.unwrap().is_some());
	assert!(coordinator.store.list_pending_agent_events(10).await.unwrap().is_empty());

	coordinator.wake_pending().await.unwrap();

	assert!(sent.try_recv().is_err());

	coordinator.recover_persisted().await.unwrap();

	let usage = coordinator
		.store
		.read_agent_usage_observation(
			"agent".into(),
			"opaque thread/1".into(),
			"opaque turn/1".into(),
		)
		.await
		.unwrap()
		.unwrap();

	assert_eq!(
		serde_json::from_str::<Value>(&usage.payload).unwrap()["tokenUsage"]["last"]["inputTokens"],
		1_000
	);
}

async fn fixture() -> (AgentCoordinator, UnboundedReceiver<Value>, TempDir) {
	fixture_with_history(serde_json::json!({})).await
}

// Deliver through the actual native transport so response guards have real evidence.
async fn attach_request_transport(
	agent: &mut AgentCoordinator,
	history: Value,
	request: Value,
) -> UnboundedReceiver<Value> {
	let (local, mut remote) = io::duplex(65_536);
	let (reader, writer) = io::split(local);
	let (client, mut events) = AppServerClient::from_io(reader, writer);

	agent.client = client;

	let (sent, received) = mpsc::unbounded_channel();
	let frames = request.as_array().cloned().unwrap_or_else(|| vec![request]);
	let count = frames.len();

	tokio::spawn(async move {
		for frame in frames {
			remote.write_all(format!("{frame}\n").as_bytes()).await.unwrap();
		}

		serve_fixture(remote, history, sent).await;
	});

	for _ in 0..count {
		agent.handle_event(events.recv().await.unwrap()).await.unwrap();
	}

	tokio::spawn(async move { while events.recv().await.is_some() {} });

	received
}

async fn emit_fixture_review_events(
	request: &Value,
	history: &Value,
	turns: u64,
	writer: &mut WriteHalf<DuplexStream>,
) {
	if request["method"] == "thread/read"
		&& request["params"]["includeTurns"] != false
		&& history["_misalignment_revert_on_read"] == true
	{
		let notice =
			serde_json::json!({"method":"thread/reverted","params":{"threadId":"opaque thread/1"}});

		writer.write_all(format!("{notice}\n").as_bytes()).await.unwrap();
	}
	if request["method"] == "turn/start" && turns == 1 && history["_live_misalignment"].is_object()
	{
		let notification = serde_json::json!({"method":"error","params":{"threadId":"opaque thread/1","turnId":"opaque turn/1","willRetry":false,"error":history["_live_misalignment"]}});

		writer.write_all(format!("{notification}\n").as_bytes()).await.unwrap();
	}
}

async fn serve_fixture(server_io: DuplexStream, history: Value, sent: UnboundedSender<Value>) {
	let (reader, mut writer) = io::split(server_io);
	let mut lines = BufReader::new(reader).lines();
	let mut threads = 0;
	let mut turns = 0;
	let mut settings = std::collections::HashMap::<String, Value>::new();
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
				serde_json::json!({"data":if request["params"]["archived"]==faults.archived {vec![serde_json::json!({"id":"opaque thread/1"})]} else {vec![]},"nextCursor":null}),
			Some("thread/unarchive") => {
				faults.archived = false;

				serde_json::json!({"thread":{"id":request["params"]["threadId"]}})
			},
			Some("turn/steer") => serde_json::json!({"turnId":request["params"]["expectedTurnId"]}),
			Some("thread/backgroundTerminals/list") => history["_background"].clone(),
			Some("thread/backgroundTerminals/terminate") =>
				serde_json::json!({"terminated":history["_terminated"]}),
			Some("thread/search") => history["_search"].clone(),
			Some("thread/searchOccurrences") => history["_occurrences"].clone(),
			Some("thread/goal/get") => serde_json::json!({"goal":history["_goal"]}),
			Some("thread/read") => fixture_thread_read(&request, &history, &settings, turns),
			Some("thread/turns/list") => {
				let id = request["params"]["threadId"].as_str().unwrap();
				let mut turns = history[id]["thread"]["turns"].clone();

				for turn in turns.as_array_mut().unwrap() {
					if request["params"]["itemsView"] == "full" {
						turn["itemsView"] = serde_json::json!("full");
					} else {
						turn["items"] = serde_json::json!([]);
						turn["itemsView"] = serde_json::json!("notLoaded");
					}
				}

				serde_json::json!({"data":turns,"nextCursor":null})
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
					.map(|item| serde_json::json!({"turnId":turn_id,"item":item}))
					.collect();

				serde_json::json!({"data":entries,"nextCursor":null})
			},
			Some("thread/resume") => {
				let id = request["params"]["threadId"].as_str().unwrap();
				let configured = settings.get(id).cloned().unwrap_or_else(
					|| serde_json::json!({"model":"selected-model","reasoningEffort":"high"}),
				);

				serde_json::json!({"thread":{"id":id,"turns":if request["params"]["excludeTurns"] == true { serde_json::json!([]) } else { history[id]["thread"]["turns"].clone() }},"model":configured["model"],"reasoningEffort":configured["reasoningEffort"]})
			},
			Some("thread/start") => {
				threads += 1;

				let id = format!("opaque thread/{threads}");
				let configured = serde_json::json!({"model":request["params"]["model"],"reasoningEffort":request["params"]["config"]["model_reasoning_effort"],"modelProvider":"openai"});

				settings.insert(id.clone(), configured.clone());

				serde_json::json!({"thread":{"id":id},"model":configured["model"],"reasoningEffort":configured["reasoningEffort"]})
			},
			Some("turn/start" | "thread/settings/update") => {
				let id = request["params"]["threadId"].as_str().unwrap();

				if let Some(configured) = settings.get_mut(id) {
					for (parameter, field) in [("model", "model"), ("effort", "reasoningEffort")] {
						if let Some(value) = request["params"].get(parameter) {
							configured[field] = value.clone();
						}
					}
				}

				if request["method"] == "turn/start" {
					turns += 1;

					serde_json::json!({"turn":{"id":format!("opaque turn/{turns}")}})
				} else {
					serde_json::json!({})
				}
			},
			_ => serde_json::json!({}),
		};

		emit_fixture_review_events(&request, &history, turns, &mut writer).await;

		let frame = format!("{}\n", serde_json::json!({"id":request["id"],"result":result}));

		writer.write_all(frame.as_bytes()).await.unwrap();
	}
}

#[tokio::test]
async fn unloaded_thread_resumes_exact_identity_without_new_thread() {
	let (mut coordinator, mut sent, _directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();

	complete(&mut coordinator, "agent").await;

	let original = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();

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
			params: serde_json::json!({"threadId":original.codex_thread_id}),
		})
		.await
		.unwrap();
	coordinator.continue_worker("agent", "Continue the original Agent").await.unwrap();

	let mut resumes = Vec::new();

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "thread/start");

		if request["method"] == "thread/resume" {
			resumes.push(request);
		}
	}

	assert_eq!(resumes.len(), 1);
	assert_eq!(resumes[0]["params"]["threadId"], serde_json::json!(original.codex_thread_id));

	for field in [
		"approvalPolicy",
		"sandbox",
		"cwd",
		"dynamicTools",
		"model",
		"config",
		"developerInstructions",
	] {
		assert!(resumes[0]["params"].get(field).is_none(), "resume must preserve {field}");
	}
}

#[tokio::test]
async fn automation_delivery_is_deduplicated_across_later_agent_turns() {
	let (mut coordinator, _sent, _directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();

	complete(&mut coordinator, "agent").await;

	coordinator
		.ingest_automation_result(
			"feed:event:1",
			"agent",
			serde_json::json!({"result":"review requested"}),
		)
		.await
		.unwrap();

	let first = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();

	assert!(first.active_turn_id.is_some());

	coordinator
		.ingest_automation_result(
			"feed:event:1",
			"agent",
			serde_json::json!({"result":"review requested"}),
		)
		.await
		.unwrap();

	assert_eq!(
		coordinator.store.get_agent_work_item("agent".into()).await.unwrap().active_turn_id,
		first.active_turn_id
	);

	complete(&mut coordinator, "agent").await;

	coordinator
		.ingest_automation_result(
			"feed:event:1",
			"agent",
			serde_json::json!({"result":"review requested"}),
		)
		.await
		.unwrap();

	assert!(
		coordinator
			.store
			.get_agent_work_item("agent".into())
			.await
			.unwrap()
			.active_turn_id
			.is_none()
	);
	assert_eq!(
		coordinator
			.store
			.list_pending_agent_events(100)
			.await
			.unwrap()
			.iter()
			.filter(|event| event.event_kind == "automation_result")
			.count(),
		1
	);
}

#[tokio::test]
async fn failed_dependency_write_cannot_leave_dispatchable_work() {
	let (mut coordinator, mut sent, directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();

	complete(&mut coordinator, "agent").await;

	coordinator.create_goal("agent", "ready", "Accepted result").await.unwrap();
	coordinator.create_goal("agent", "waiting", "Unresolved result").await.unwrap();
	coordinator
		.store
		.set_agent_work_status("ready".into(), AgentWorkStatus::Resolved, None)
		.await
		.unwrap();

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
	let db = Connection::open(root.paths().product_database_file()).unwrap();

	db.execute_batch("CREATE TRIGGER fail_second_dependency BEFORE INSERT ON agent_dependencies WHEN NEW.work_item_id='dependent' AND NEW.depends_on_id='waiting' BEGIN SELECT RAISE(ABORT, 'fixture dependency write failure'); END;").unwrap();

	while sent.try_recv().is_ok() {}

	assert!(
		coordinator
			.create_worker_with_dependencies(
				"agent",
				"dependent",
				"Use both results",
				vec!["ready".into(), "waiting".into()]
			)
			.await
			.is_err()
	);

	coordinator.wake_pending().await.unwrap();

	let requests: Vec<_> = iter::from_fn(|| sent.try_recv().ok()).collect();

	assert!(
		!requests.iter().any(|request| matches!(
			request["method"].as_str(),
			Some("thread/start" | "turn/start")
		)),
		"failed work creation dispatched native requests: {requests:?}"
	);
	assert!(coordinator.store.get_agent_work_item("dependent".into()).await.is_err());
	assert!(coordinator.store.list_agent_dependencies().await.unwrap().is_empty());

	db.execute_batch("DROP TRIGGER fail_second_dependency;").unwrap();

	let work = coordinator
		.create_worker_with_dependencies(
			"agent",
			"dependent",
			"Use both results",
			vec!["ready".into(), "waiting".into()],
		)
		.await
		.unwrap();

	assert!(work.codex_thread_id.is_none());
	assert_eq!(coordinator.store.list_agent_dependencies().await.unwrap().len(), 2);
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn dependencies_block_turns_until_explicit_resolution() {
	let (mut coordinator, mut sent, _directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();
	coordinator.create_worker("agent", "first", "Inspect").await.unwrap();

	let blocked = coordinator
		.create_worker_with_dependencies("agent", "second", "Use inspection", vec!["first".into()])
		.await
		.unwrap();

	assert!(blocked.active_turn_id.is_none());
	assert!(blocked.codex_thread_id.is_none());

	while sent.try_recv().is_ok() {}

	assert!(
		matches!(coordinator.continue_worker("second","Proceed").await,Err(AgentError::DependenciesPending(ids)) if ids==vec!["first"])
	);
	assert!(sent.try_recv().is_err());

	complete(&mut coordinator, "first").await;

	assert!(matches!(
		coordinator.continue_worker("second", "Proceed").await,
		Err(AgentError::DependenciesPending(_))
	));

	coordinator
		.store
		.set_agent_work_status("first".into(), AgentWorkStatus::Resolved, None)
		.await
		.unwrap();

	let mut coordinator = AgentCoordinator::new(
		coordinator.store.clone(),
		coordinator.client.clone(),
		coordinator.config.clone(),
	)
	.unwrap();

	coordinator.continue_worker("second", "Proceed with accepted inspection").await.unwrap();

	assert!(
		coordinator
			.store
			.get_agent_work_item("second".into())
			.await
			.unwrap()
			.active_turn_id
			.is_some()
	);
}

#[tokio::test]
async fn wait_requires_future_due_and_due_checks_wake_once_per_timestamp() {
	let (mut coordinator, _sent, _directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();

	complete(&mut coordinator, "agent").await;

	coordinator
		.ingest_automation_result("source:wait", "agent", serde_json::json!({"result":"not ready"}))
		.await
		.unwrap();

	let agent = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let event = coordinator
		.store
		.list_pending_agent_events(100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.event_kind == "automation_result")
		.unwrap();
	let mut args = serde_json::json!({"id":"agent","eventIds":[event.id],"status":"wait","summary":"Check source again"});

	assert!(
		coordinator
			.tool(&agent, &serde_json::json!({"tool":"agent_disposition","arguments":args}))
			.await
			.is_err()
	);

	let due = agent::now_micros().unwrap() + 60_000_000;

	args["nextCheckAtMicros"] = serde_json::json!(due);

	coordinator
		.tool(&agent, &serde_json::json!({"tool":"agent_disposition","arguments":args}))
		.await
		.unwrap();

	complete(&mut coordinator, "agent").await;

	coordinator.check_due_followups(due - 1).await.unwrap();

	assert!(
		coordinator
			.store
			.get_agent_work_item("agent".into())
			.await
			.unwrap()
			.active_turn_id
			.is_none()
	);

	coordinator.check_due_followups(due).await.unwrap();

	assert!(
		coordinator
			.store
			.get_agent_work_item("agent".into())
			.await
			.unwrap()
			.active_turn_id
			.is_some()
	);

	complete(&mut coordinator, "agent").await;

	coordinator.check_due_followups(due + 1).await.unwrap();

	assert!(
		coordinator
			.store
			.get_agent_work_item("agent".into())
			.await
			.unwrap()
			.active_turn_id
			.is_none()
	);
	assert_eq!(
		coordinator
			.store
			.list_pending_agent_events(100)
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
	let (mut coordinator, _sent, directory) = fixture().await;

	coordinator.client.shutdown().await.unwrap();

	assert!(coordinator.start_agent("agent", "Coordinate").await.is_err());

	let work = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Unknown);
	assert!(work.codex_thread_id.is_none());

	coordinator.recover_persisted().await.unwrap();

	assert_eq!(
		coordinator.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Unknown
	);
	assert!(matches!(
		coordinator.continue_worker("agent", "Retry").await,
		Err(AgentError::UnknownDispatch)
	));

	let paths =
		DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap().paths();

	drop(coordinator);

	let reopened = SqliteStore::open(&paths).unwrap();
	let (mut cold, mut sent, _fresh_directory) = fixture().await;

	cold.store = reopened;

	cold.recover_persisted().await.unwrap();

	let root = cold.store.get_agent_work_item("agent".into()).await.unwrap();

	assert!(root.codex_thread_id.is_none());
	assert_eq!(root.instructions, "Coordinate");
	assert_eq!(root.dispatch_state, decodex_database::AgentDispatchState::Unknown);
	assert!(matches!(
		cold.start_reserved_agent("agent", "Retry after restart").await,
		Err(AgentError::UnknownDispatch)
	));
	assert!(cold.start_agent("replacement", "Duplicate root").await.is_err());
	assert!(
		sent.try_recv().is_err(),
		"a fresh native connection must not materialize an uncertain root again"
	);
}

#[tokio::test]
async fn recovery_records_only_exact_terminal_evidence_without_dispatching() {
	let history = serde_json::json!({
		"opaque thread/1":{"thread":{"id":"opaque thread/1","status":{"type":"idle"},"turns":[{"id":"opaque turn/1","status":"completed","items":[{"type":"agentMessage","text":"Agent waiting"}]}]}},
		"opaque thread/2":{"thread":{"id":"opaque thread/2","status":{"type":"idle"},"turns":[{"id":"opaque turn/2","status":"failed","items":[{"type":"agentMessage","text":"Worker exact evidence"}]}]}}
	});
	let (mut coordinator, mut sent, _directory) = fixture_with_history(history).await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();
	coordinator.create_goal("agent", "goal", "Goal").await.unwrap();
	coordinator.create_worker("goal", "worker", "Inspect").await.unwrap();

	while sent.try_recv().is_ok() {}

	let mut recovered = AgentCoordinator::new(
		coordinator.store.clone(),
		coordinator.client.clone(),
		coordinator.config.clone(),
	)
	.unwrap();

	recovered.recover_persisted().await.unwrap();

	for id in ["agent", "worker"] {
		assert_eq!(
			recovered.store.get_agent_work_item(id.into()).await.unwrap().dispatch_state,
			decodex_database::AgentDispatchState::Idle
		);
	}

	let events = recovered.store.list_pending_agent_events(100).await.unwrap();

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_kind, "worker_turn_completed");
	assert!(events[0].payload.contains("Worker exact evidence"));
	assert!(events[0].payload.contains("failed"));

	while let Ok(request) = sent.try_recv() {
		assert!(["thread/resume", "thread/read"].contains(&request["method"].as_str().unwrap()));
	}

	recovered.recover_persisted().await.unwrap();

	while let Ok(request) = sent.try_recv() {
		assert_eq!(
			request["method"], "thread/read",
			"repeat recovery only checks existing native history"
		);
	}

	assert_eq!(recovered.store.list_pending_agent_events(100).await.unwrap().len(), 1);
}

#[tokio::test]
async fn recovery_missing_exact_turn_preserves_unknown_without_replay() {
	let history = serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"different-turn","status":"completed","items":[]}]}}});
	let (mut coordinator, mut sent, _directory) = fixture_with_history(history).await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	coordinator.recover_persisted().await.unwrap();

	let work = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Unknown);
	assert_eq!(work.active_turn_id.as_deref(), Some("opaque turn/1"));
	assert!(coordinator.store.list_pending_agent_events(100).await.unwrap().is_empty());

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start");
		assert_ne!(request["method"], "thread/start");
	}

	assert!(matches!(
		coordinator.continue_worker("agent", "Do not replay").await,
		Err(AgentError::UnknownDispatch)
	));
}

#[tokio::test]
async fn recovery_preserves_positive_active_turn_observation() {
	let history = serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","status":{"type":"active","activeFlags":[]},"turns":[{"id":"opaque turn/1","status":"inProgress","items":[]}]}}});
	let (mut coordinator, mut sent, _directory) = fixture_with_history(history).await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	coordinator.recover_persisted().await.unwrap();

	assert_eq!(
		coordinator.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Running
	);

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start");
	}
}

#[tokio::test]
async fn initial_user_input_starts_once_and_receipt_ack_leaves_newer_input_pending() {
	let (mut coordinator, mut sent, _directory) = fixture().await;

	AgentCoordinator::reserve_root(&coordinator.store, "agent", "Personal Agent").await.unwrap();

	coordinator
		.enqueue_user_message("agent", "command-1", "Handle my actual request")
		.await
		.unwrap();
	coordinator.wake_pending().await.unwrap();
	coordinator.wake_pending().await.unwrap();

	let work = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();

	coordinator.enqueue_user_message("agent", "command-2", "A later request").await.unwrap();
	coordinator.record_terminal(serde_json::json!({"threadId":work.codex_thread_id,"turn":{"id":work.active_turn_id,"status":"completed","items":[]}}),Ok(serde_json::json!({})),true).await.unwrap();

	let pending = coordinator.store.list_pending_agent_events(100).await.unwrap();

	assert_eq!(pending.len(), 1);
	assert!(pending[0].payload.contains("A later request"));
	assert!(pending[0].delivered_turn_id.is_none());

	let starts: Vec<_> = iter::from_fn(|| sent.try_recv().ok())
		.filter(|request| request["method"] == "turn/start")
		.collect();

	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["input"][0]["text"], "Handle my actual request");

	coordinator.wake_pending().await.unwrap();

	complete(&mut coordinator, "agent").await;

	coordinator
		.enqueue_user_message("agent", "command-1", "Handle my actual request")
		.await
		.unwrap();
	coordinator.wake_pending().await.unwrap();

	assert!(
		coordinator
			.store
			.get_agent_work_item("agent".into())
			.await
			.unwrap()
			.active_turn_id
			.is_none()
	);
	assert!(coordinator.store.list_pending_agent_events(100).await.unwrap().is_empty());
}

#[tokio::test]
async fn large_requested_decisions_reach_native_once_without_truncation() {
	for (method, requested, decision) in [
		(
			"item/permissions/requestApproval",
			serde_json::json!({"permissions":{"fileSystem":{"write":["/tmp/界".repeat(10_000)]}}}),
			AgentRequestedDecision::PermissionsForTurn,
		),
		(
			"item/commandExecution/requestApproval",
			serde_json::json!({"command":"fixture","availableDecisions":["decline",{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["large-argument".repeat(8_000)]}}]}),
			AgentRequestedDecision::CommandPolicy { index: 1 },
		),
	] {
		let (mut agent, _old_sent, _directory) = fixture().await;
		let root = agent.start_agent("agent", "Coordinate").await.unwrap();
		let mut params = requested;

		params["threadId"] = serde_json::json!(root.codex_thread_id);
		params["turnId"] = serde_json::json!(root.active_turn_id);
		params["itemId"] = serde_json::json!("large-selected-decision");

		let mut sent = attach_request_transport(
			&mut agent,
			serde_json::json!({}),
			serde_json::json!({"id":7,"method":method,"params":params}),
		)
		.await;
		let event = agent
			.store
			.list_pending_agent_events(100)
			.await
			.unwrap()
			.into_iter()
			.find(|event| event.event_kind == "permission_pending")
			.unwrap();

		assert!(event.payload.len() < 4_096);

		let stored = agent.store.get_agent_inbox_event(event.id).await.unwrap();
		let payload: Value = serde_json::from_str(&stored.payload).unwrap();
		let response =
			decodex_protocol::requested_decision_response(method, &payload["params"], &decision)
				.unwrap();

		assert!(response.to_string().len() > decodex_protocol::MAX_HISTORY_INLINE_BYTES);

		agent.respond_pending_event(event.id, response.clone()).await.unwrap();

		assert_eq!(sent.recv().await.unwrap(), serde_json::json!({"id":7,"result":response}));
		assert!(agent.respond_pending_event(event.id, response).await.is_err());
		assert!(sent.try_recv().is_err());
		assert!(agent.store.get_agent_inbox_event(event.id).await.unwrap().disposition.is_some());
	}
}

#[tokio::test]
async fn permission_response_uses_live_event_identity_even_when_rpc_id_is_reused() {
	let (mut coordinator, _old_sent, _directory) = fixture().await;
	let root = coordinator.start_agent("agent", "Coordinate").await.unwrap();
	let params = serde_json::json!({"threadId":root.codex_thread_id,"turnId":root.active_turn_id,
		"command": "true # 界".repeat(10_000), "availableDecisions":["accept","decline"]});

	coordinator
		.handle_event(ServerEvent::Request {
			id: RequestId::Number(7),
			method: "item/commandExecution/requestApproval".into(),
			params: params.clone(),
		})
		.await
		.unwrap();

	let old_id = coordinator.store.list_pending_agent_events(100).await.unwrap()[0].id;
	let mut reconnected = AgentCoordinator::new(
		coordinator.store.clone(),
		coordinator.client.clone(),
		coordinator.config.clone(),
	)
	.unwrap();
	let mut sent = attach_request_transport(
		&mut reconnected,
		serde_json::json!({}),
		serde_json::json!({"id":7,"method":"item/commandExecution/requestApproval","params":params}),
	)
	.await;
	let new_id = reconnected
		.store
		.list_pending_agent_events(100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.id != old_id)
		.unwrap()
		.id;

	while sent.try_recv().is_ok() {}

	assert!(
		reconnected
			.respond_pending_event(old_id, serde_json::json!({"decision":"decline"}))
			.await
			.is_err()
	);
	assert!(sent.try_recv().is_err());

	reconnected
		.respond_pending_event(new_id, serde_json::json!({"decision":"decline"}))
		.await
		.unwrap();

	let response = sent.recv().await.unwrap();

	assert_eq!(response, serde_json::json!({"id":7,"result":{"decision":"decline"}}));
	assert!(
		reconnected
			.respond_pending_event(new_id, serde_json::json!({"decision":"decline"}))
			.await
			.is_err()
	);
	assert_eq!(
		reconnected.store.get_agent_work_item("agent".into()).await.unwrap().status,
		AgentWorkStatus::Open
	);

	let pending = reconnected.store.list_pending_agent_events(100).await.unwrap();

	assert_eq!(pending.len(), 1);
	assert_eq!(pending[0].id, old_id);
}

#[tokio::test]
async fn resolving_prerequisite_releases_authorized_unbound_worker_once() {
	let (mut coordinator, mut sent, _directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();
	coordinator.create_worker("agent", "first", "Inspect").await.unwrap();
	coordinator
		.create_worker_with_dependencies("agent", "second", "Use the result", vec!["first".into()])
		.await
		.unwrap();

	complete(&mut coordinator, "agent").await;
	complete(&mut coordinator, "first").await;

	let agent = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let event = coordinator
		.store
		.list_agent_events_for_turn(agent.active_turn_id.clone().unwrap(), 100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.work_item_id == "first")
		.unwrap();

	while sent.try_recv().is_ok() {}

	let result=coordinator.tool(&agent,&serde_json::json!({"tool":"agent_disposition","arguments":{"id":"first","status":"resolved","summary":"Evidence accepted","eventIds":[event.id]}})).await.unwrap();

	assert_eq!(result["releasedWorkIds"], serde_json::json!(["second"]));

	let second = coordinator.store.get_agent_work_item("second".into()).await.unwrap();

	assert!(second.codex_thread_id.is_some());
	assert!(second.active_turn_id.is_some());
	assert!(coordinator.release_ready_workers("agent").await.unwrap().is_empty());

	let requests: Vec<_> = iter::from_fn(|| sent.try_recv().ok()).collect();

	assert_eq!(requests.iter().filter(|request| request["method"] == "thread/start").count(), 1);
	assert_eq!(requests.iter().filter(|request| request["method"] == "turn/start").count(), 1);

	let turn = requests.iter().find(|request| request["method"] == "turn/start").unwrap();

	assert_eq!(turn["params"]["input"], serde_json::json!([]));
	assert_eq!(
		turn["params"]["toolOutput"],
		serde_json::json!({"name":"work_instruction","namespace":"decodex","output":"Use the result"})
	);
}

async fn complete(coordinator: &mut AgentCoordinator, id: &str) {
	let work = coordinator.store.get_agent_work_item(id.into()).await.unwrap();

	coordinator.handle_event(ServerEvent::Notification {
        method:"turn/completed".into(),params:serde_json::json!({"threadId":work.codex_thread_id,"turn":{"id":work.active_turn_id,"status":"completed","items":[{"type":"agentMessage","text":"result"}]}})
    }).await.unwrap();
}

#[tokio::test]
async fn goal_completion_requires_explicit_judgment_and_related_evidence() {
	let (mut coordinator, _sent, _directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();
	coordinator.create_goal("agent", "goal", "Outcome").await.unwrap();
	coordinator.create_goal("agent", "other", "Different outcome").await.unwrap();
	coordinator.create_worker("goal", "worker", "Inspect").await.unwrap();

	complete(&mut coordinator, "agent").await;
	complete(&mut coordinator, "worker").await;

	let agent = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let event = coordinator
		.store
		.list_agent_events_for_turn(agent.active_turn_id.clone().unwrap(), 100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.work_item_id == "worker")
		.unwrap();

	coordinator.tool(&agent,&serde_json::json!({"tool":"agent_disposition","arguments":{"id":"worker","status":"resolved","eventIds":[event.id],"summary":"Result accepted"}})).await.unwrap();

	assert_eq!(
		coordinator.store.get_agent_work_item("goal".into()).await.unwrap().status,
		AgentWorkStatus::Open
	);

	let command = |id| serde_json::json!({"tool":"agent_resolve_goal","arguments":{"id":id,"evidenceEventId":event.id,"summary":"The accepted evidence satisfies this goal"}});

	assert!(coordinator.tool(&agent, &command("other")).await.is_err());

	coordinator.tool(&agent, &command("goal")).await.unwrap();

	assert_eq!(
		coordinator.store.get_agent_work_item("goal".into()).await.unwrap().status,
		AgentWorkStatus::Resolved
	);
	assert!(
		coordinator
			.store
			.read_agent_work_events("goal".into(), 10)
			.await
			.unwrap()
			.iter()
			.any(|entry| entry.event_kind == "goal_resolved")
	);
	assert!(coordinator.tool(&agent, &command("goal")).await.is_err());
}

#[tokio::test]
async fn explicit_user_reply_resolves_worker_decision_without_rewriting_old_evidence() {
	let (mut coordinator, _sent, _directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();
	coordinator.create_worker("agent", "worker", "Inspect").await.unwrap();

	complete(&mut coordinator, "agent").await;
	complete(&mut coordinator, "worker").await;

	let agent = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let event = coordinator
		.store
		.list_agent_events_for_turn(agent.active_turn_id.clone().unwrap(), 100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.work_item_id == "worker")
		.unwrap();

	coordinator.tool(&agent,&serde_json::json!({"tool":"agent_disposition","arguments":{"id":"worker","status":"user_decision","summary":"Choose A or B","eventIds":[event.id]}})).await.unwrap();

	complete(&mut coordinator, "agent").await;

	coordinator
		.enqueue_user_message("agent", "answer-once", "Choose A and accept this result")
		.await
		.unwrap();
	coordinator.wake_pending().await.unwrap();

	let agent = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let user = coordinator
		.store
		.list_agent_events_for_turn(agent.active_turn_id.clone().unwrap(), 100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.event_kind == "user_message")
		.unwrap();
	let command = |user_id| serde_json::json!({"tool":"agent_resolve_decision","arguments":{"id":"worker","userEventId":user_id,"summary":"User selected A; result accepted"}});

	assert!(coordinator.tool(&agent, &command(event.id)).await.is_err());

	coordinator.tool(&agent, &command(user.id)).await.unwrap();

	assert_eq!(
		coordinator.store.get_agent_work_item("worker".into()).await.unwrap().status,
		AgentWorkStatus::Resolved
	);
	assert_eq!(
		coordinator.store.get_agent_inbox_event(event.id).await.unwrap().disposition,
		Some(AgentDisposition::UserDecision)
	);

	let evidence = coordinator.store.read_agent_work_events("worker".into(), 100).await.unwrap();

	assert!(evidence.iter().any(|item| item.event_kind == "user_decision_resolved"
		&& item.payload.contains(&format!("\"userEventId\":{}", user.id))));
	assert!(coordinator.tool(&agent, &command(user.id)).await.is_err());
	assert!(coordinator.store.get_agent_inbox_event(user.id).await.unwrap().disposition.is_none());
}

#[tokio::test]
async fn independent_workers_queue_then_wake_and_continue_same_identity() {
	let (mut coordinator, mut sent, _directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate two workers").await.unwrap();
	coordinator.create_goal("agent", "goal-a", "Goal A").await.unwrap();
	coordinator.create_goal("agent", "goal-b", "Goal B").await.unwrap();

	let first = coordinator.create_worker("goal-a", "first", "Inspect A").await.unwrap();
	let second = coordinator.create_worker("goal-b", "second", "Inspect B").await.unwrap();

	assert_ne!(first.codex_thread_id, second.codex_thread_id);

	complete(&mut coordinator, "first").await;

	assert_eq!(
		coordinator
			.store
			.get_agent_work_item("agent".into())
			.await
			.unwrap()
			.active_turn_id
			.as_deref(),
		Some("opaque turn/1")
	);

	complete(&mut coordinator, "agent").await;

	let resumed = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(resumed.active_turn_id.as_deref(), Some("opaque turn/4"));

	coordinator.continue_worker("first", "Repair missing evidence").await.unwrap();

	assert_eq!(
		coordinator.store.get_agent_work_item("first".into()).await.unwrap().codex_thread_id,
		first.codex_thread_id
	);

	complete(&mut coordinator, "agent").await;

	coordinator.wake_pending().await.unwrap();

	assert!(
		coordinator
			.store
			.get_agent_work_item("agent".into())
			.await
			.unwrap()
			.active_turn_id
			.is_none()
	);

	complete(&mut coordinator, "second").await;

	assert!(
		coordinator
			.store
			.get_agent_work_item("agent".into())
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
	assert_eq!(starts[4]["params"]["threadId"], serde_json::json!(first.codex_thread_id));
	assert_eq!(starts[1]["params"]["effort"], "medium");
	assert_eq!(starts[3]["params"]["threadId"], serde_json::json!(resumed.codex_thread_id));
}

#[tokio::test]
async fn disposition_cannot_consume_undelivered_events_and_requests_stay_pending() {
	let (mut coordinator, mut sent, _directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();
	coordinator.create_worker("agent", "first", "Inspect").await.unwrap();

	complete(&mut coordinator, "agent").await;
	complete(&mut coordinator, "first").await;

	let agent = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let delivered = coordinator
		.store
		.list_pending_agent_events(100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.work_item_id == "first")
		.unwrap();
	let newer = coordinator
		.store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "automation:new".into(),
			work_item_id: "first".into(),
			event_kind: "automation_result".into(),
			payload: "new facts".into(),
		})
		.await
		.unwrap();
	let params = serde_json::json!({"tool":"agent_disposition","arguments":{"id":"first","status":"resolved","summary":"Reviewed result","eventIds":[newer.id]}});

	assert!(coordinator.tool(&agent, &params).await.is_err());

	let params = serde_json::json!({"tool":"agent_disposition","arguments":{"id":"first","status":"resolved","summary":"Reviewed result","eventIds":[delivered.id]}});

	coordinator.tool(&agent, &params).await.unwrap();

	assert!(
		coordinator
			.store
			.list_pending_agent_events(100)
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
			params: serde_json::json!({"threadId":agent.codex_thread_id}),
		})
		.await
		.unwrap();

	assert!(sent.try_recv().is_err());
	assert!(
		coordinator
			.store
			.list_pending_agent_events(100)
			.await
			.unwrap()
			.iter()
			.any(|event| event.event_kind == "permission_pending")
	);
}

#[tokio::test]
async fn unknown_dispatch_and_unprocessed_events_survive_restart() {
	let (mut coordinator, _sent, directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();
	coordinator.create_worker("agent", "first", "Inspect").await.unwrap();

	complete(&mut coordinator, "first").await;

	let paths =
		DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap().paths();

	coordinator.store.begin_agent_dispatch("first".into()).await.unwrap();
	coordinator.store.mark_agent_dispatch_unknown("first".into()).await.unwrap();

	drop(coordinator);

	let reopened = SqliteStore::open(&paths).unwrap();

	assert!(reopened.begin_agent_dispatch("first".into()).await.is_err());
	assert_eq!(reopened.list_pending_agent_events(100).await.unwrap().len(), 1);
}

#[tokio::test]
async fn live_output_is_turn_bound_bounded_and_replaced_by_final_history() {
	let (mut coordinator, _sent, _directory) = fixture().await;
	let work = coordinator.start_agent("agent", "Talk").await.unwrap();
	let event = |turn: &str, text: &str| ServerEvent::Notification {
		method: "item/agentMessage/delta".into(),
		params: serde_json::json!({"threadId":work.codex_thread_id,"turnId":turn,"itemId":"answer","delta":text}),
	};
	let turn = work.active_turn_id.as_deref().unwrap();

	coordinator.handle_event(event("old-turn", "wrong")).await.unwrap();

	assert!(coordinator.store.read_agent_output("agent".into()).await.unwrap().is_empty());

	let (revision, initial) =
		coordinator.store.wait_agent_output("agent".into(), None).await.unwrap();

	assert!(initial.is_empty());

	let store = coordinator.store.clone();
	let mut observer =
		tokio::spawn(async move { store.wait_agent_output("agent".into(), Some(revision)).await });

	assert!(
		time::timeout(std::time::Duration::from_millis(10), &mut observer).await.is_err(),
		"unchanged output must remain asleep"
	);

	coordinator.handle_event(event(turn, "Hello ")).await.unwrap();

	let (next, observed) =
		time::timeout(Duration::from_secs(1), observer).await.unwrap().unwrap().unwrap();

	assert!(next > revision);
	assert_eq!(observed[0].text, "Hello ");

	coordinator.handle_event(event(turn, "世界")).await.unwrap();

	let live = coordinator.store.read_agent_output("agent".into()).await.unwrap();

	assert_eq!(live[0].text, "Hello 世界");
	assert!(
		coordinator
			.store
			.list_pending_agent_events(100)
			.await
			.unwrap()
			.iter()
			.all(|event| event.event_kind != "assistant_delta")
	);

	coordinator.handle_event(event(turn, &"界".repeat(30_000))).await.unwrap();

	let live = coordinator.store.read_agent_output("agent".into()).await.unwrap();

	assert!(live[0].truncated);
	assert!(live[0].text.len() <= 65_536);

	complete(&mut coordinator, "agent").await;

	assert!(coordinator.store.read_agent_output("agent".into()).await.unwrap().is_empty());
}

#[tokio::test]
async fn nested_managers_own_their_inbox_tools_and_workspace_directory() {
	let (mut coordinator, mut sent, directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();

	let manager = coordinator
		.create_manager(
			"agent",
			"project",
			"Manage project",
			Some(("Project".into(), directory.path().display().to_string())),
		)
		.await
		.unwrap();

	coordinator.create_manager("project", "team", "Manage team", None).await.unwrap();
	coordinator.create_worker("team", "worker", "Do work").await.unwrap();

	let root = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let denied = coordinator.tool(&root,&serde_json::json!({"tool":"agent_continue_worker","arguments":{"id":"worker","prompt":"Bypass its manager"}})).await;

	assert!(denied.is_err());

	let visible = coordinator
		.tool(&manager, &serde_json::json!({"tool":"agent_list_work","arguments":{}}))
		.await
		.unwrap();

	assert!(
		visible["work"]
			.as_array()
			.unwrap()
			.iter()
			.all(|work| work["id"] != "agent" && work["id"] != "worker")
	);

	complete(&mut coordinator, "worker").await;

	assert!(
		coordinator.store.list_agent_wake_events("agent".into(), 100).await.unwrap().is_empty()
	);
	assert!(
		coordinator.store.list_agent_wake_events("project".into(), 100).await.unwrap().is_empty()
	);

	let events = coordinator.store.list_agent_wake_events("team".into(), 100).await.unwrap();

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].work_item_id, "worker");

	let requests: Vec<_> = iter::from_fn(|| sent.try_recv().ok()).collect();
	let starts: Vec<_> =
		requests.iter().filter(|request| request["method"] == "thread/start").collect();

	assert_eq!(starts.len(), 4);
	assert_eq!(starts[0]["params"]["threadSource"], "user");
	assert!(starts[1..].iter().all(|start| start["params"].get("threadSource").is_none()));
	assert!(starts[1]["params"]["dynamicTools"].is_array());
	assert!(starts[2]["params"]["dynamicTools"].is_array());
	assert!(starts[3]["params"].get("dynamicTools").is_none());
	assert_eq!(
		starts[3]["params"]["cwd"],
		serde_json::json!(directory.path().canonicalize().unwrap())
	);
}

#[tokio::test]
async fn legacy_manager_keeps_native_thread_without_replaying_saved_input() {
	let (mut coordinator, mut sent, directory) = fixture().await;
	let original = coordinator.start_agent("agent", "Original request").await.unwrap();

	complete(&mut coordinator, "agent").await;

	coordinator
		.store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "old-user".into(),
			work_item_id: "agent".into(),
			event_kind: "user_message".into(),
			payload: serde_json::json!({"text":"Remember the existing project"}).to_string(),
		})
		.await
		.unwrap();

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
	let db = Connection::open(root.paths().product_database_file()).unwrap();

	db.execute("DELETE FROM agent_tool_versions WHERE work_id='agent'", []).unwrap();

	while sent.try_recv().is_ok() {}

	coordinator.continue_worker("agent", "One new request").await.unwrap();

	let upgraded = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(original.codex_thread_id, upgraded.codex_thread_id);
	assert_eq!(coordinator.store.agent_tool_version("agent".into()).await.unwrap(), 1);

	let messages: Vec<_> = iter::from_fn(|| sent.try_recv().ok()).collect();

	assert_eq!(messages.iter().filter(|message| message["method"] == "thread/start").count(), 0);
	assert_eq!(messages.iter().filter(|message| message["method"] == "turn/start").count(), 1);
	assert!(!messages.iter().any(|message| message["method"] == "thread/inject_items"));

	let start = messages.iter().find(|message| message["method"] == "turn/start").unwrap();

	assert_eq!(start["params"]["input"][0]["text"], "One new request");
	assert_eq!(
		db.query_row("SELECT count(*) FROM agent_thread_revisions", [], |row| row.get::<_, i64>(0))
			.unwrap(),
		0
	);
}

#[tokio::test]
async fn queued_user_messages_keep_native_turn_boundaries() {
	let (mut coordinator, mut sent, _directory) = fixture().await;

	AgentCoordinator::reserve_root(&coordinator.store, "agent", "Agent").await.unwrap();

	coordinator
		.enqueue_user_message("agent", "first", "First message\nwith a second line")
		.await
		.unwrap();
	coordinator.enqueue_user_message("agent", "second", "Second message").await.unwrap();

	let evidence = coordinator
		.store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "background-result".into(),
			work_item_id: "agent".into(),
			event_kind: "automation_result".into(),
			payload: serde_json::json!({"result":"Background evidence"}).to_string(),
		})
		.await
		.unwrap();

	coordinator.wake_pending().await.unwrap();

	let starts: Vec<_> = iter::from_fn(|| sent.try_recv().ok())
		.filter(|request| request["method"] == "turn/start")
		.collect();

	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["input"][0]["text"], "First message\nwith a second line");

	complete(&mut coordinator, "agent").await;

	coordinator.wake_pending().await.unwrap();

	let starts: Vec<_> = iter::from_fn(|| sent.try_recv().ok())
		.filter(|request| request["method"] == "turn/start")
		.collect();

	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["input"][0]["text"], "Second message");
	assert!(
		coordinator
			.store
			.get_agent_inbox_event(evidence.id)
			.await
			.unwrap()
			.delivered_turn_id
			.is_none()
	);

	let message = agent::wake_message(slice::from_ref(&evidence)).unwrap();

	assert!(!message.contains("Background evidence"));
	assert!(agent::wake_evidence(&evidence).to_string().contains("Background evidence"));
	assert!(!message.contains("source_event_id"));
	assert!(!message.contains("delivered_turn_id"));
}

#[tokio::test]
async fn usage_is_source_bound_persistent_and_does_not_wake_managers() {
	let (mut coordinator, _sent, directory) = fixture().await;
	let work = coordinator.start_agent("agent", "Talk").await.unwrap();
	let event = |turn: &str, input: i64| ServerEvent::Notification {
		method: "thread/tokenUsage/updated".into(),
		params: serde_json::json!({"threadId":work.codex_thread_id,"turnId":turn,"tokenUsage":{
			"total":{"inputTokens":input,"outputTokens":45},
			"last":{"totalTokens":1_200},"modelContextWindow":10_000}}),
	};
	let turn = work.active_turn_id.as_deref().unwrap();
	let baseline = coordinator.store.read_agent_usage("agent".into()).await.unwrap();

	coordinator.handle_event(event("wrong-turn", 300)).await.unwrap();

	assert_eq!(coordinator.store.read_agent_usage("agent".into()).await.unwrap(), baseline);

	coordinator.handle_event(event(turn, 300)).await.unwrap();
	coordinator.handle_event(event(turn, -1)).await.unwrap();

	let saved = coordinator.store.read_agent_usage("agent".into()).await.unwrap().unwrap();
	let usage: Value = serde_json::from_str(&saved).unwrap();

	assert_eq!(usage["input_tokens"], 300);
	assert_eq!(usage["output_tokens"], 45);
	assert_eq!(usage["context_tokens"], 1_200);
	assert_eq!(usage["context_window"], 10_000);
	assert!(coordinator.store.list_pending_agent_events(100).await.unwrap().is_empty());

	complete(&mut coordinator, "agent").await;

	coordinator.handle_event(event(turn, 999)).await.unwrap();

	let paths =
		DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap().paths();

	drop(coordinator);

	let reopened = SqliteStore::open(&paths).unwrap();

	assert_eq!(reopened.read_agent_usage("agent".into()).await.unwrap(), Some(saved));
}

#[tokio::test]
async fn external_writer_release_requires_a_new_send() {
	let (mut coordinator, mut sent, _directory) =
		fixture_with_history(serde_json::json!({"_resume_failures":1})).await;
	let original = coordinator.start_agent("agent", "Initial").await.unwrap();

	complete(&mut coordinator, "agent").await;

	coordinator.loaded_threads.clear();

	while sent.try_recv().is_ok() {}

	coordinator.enqueue_user_message("agent", "one-input", "Continue").await.unwrap();

	assert!(matches!(coordinator.wake_pending().await, Err(AgentError::ThreadOwnedElsewhere)));

	let before = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(before.dispatch_state, decodex_database::AgentDispatchState::Idle);

	let pending = coordinator.store.list_agent_wake_events("agent".into(), 10).await.unwrap();

	assert_eq!(pending.len(), 1);
	assert!(pending[0].delivered_turn_id.is_none());

	for index in 0..1_000 {
		coordinator
			.store
			.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id: format!("older-diagnostic-{index}"),
				work_item_id: "agent".into(),
				event_kind: "diagnostic".into(),
				payload: "{}".into(),
			})
			.await
			.unwrap();
	}

	coordinator
		.store
		.record_agent_thread_in_use("agent".into(), "Open elsewhere".into())
		.await
		.unwrap();
	coordinator.wake_pending().await.unwrap();

	assert!(coordinator.store.list_agent_wake_events("agent".into(), 10).await.unwrap().is_empty());

	let mut starts = 0;

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "thread/start");

		if request["method"] == "turn/start" {
			starts += 1;

			assert_eq!(request["params"]["threadId"].as_str(), original.codex_thread_id.as_deref());
		}
	}

	assert_eq!(starts, 0);

	coordinator.enqueue_user_message("agent", "explicit-resend", "Continue").await.unwrap();
	coordinator.wake_pending().await.unwrap();

	let requests: Vec<_> = iter::from_fn(|| sent.try_recv().ok()).collect();

	assert_eq!(requests.iter().filter(|r| r["method"] == "turn/start").count(), 1);
}

#[tokio::test]
async fn turn_usage_sums_model_calls_without_double_counting_or_using_context_as_total() {
	let (mut coordinator, _sent, _directory) = fixture().await;
	let first = coordinator.start_agent("agent", "First").await.unwrap();
	let thread = first.codex_thread_id.clone().unwrap();
	let event = |turn: &str, input: i64, output: i64, context: i64| ServerEvent::Notification {
		method: "thread/tokenUsage/updated".into(),
		params: serde_json::json!({"threadId":thread,"turnId":turn,"tokenUsage":{
			"total":{"inputTokens":input,"outputTokens":output},
			"last":{"totalTokens":context},"modelContextWindow":10_000}}),
	};
	let turn = first.active_turn_id.as_deref().unwrap();

	coordinator.handle_event(event(turn, 600, 40, 640)).await.unwrap();
	coordinator.handle_event(event(turn, 1_000, 100, 500)).await.unwrap();
	coordinator.handle_event(event(turn, 1_000, 100, 500)).await.unwrap();

	complete(&mut coordinator, "agent").await;

	let events = coordinator.store.read_agent_work_events("agent".into(), 10).await.unwrap();
	let first_usage: Value = serde_json::from_str(&events.last().unwrap().payload).unwrap();

	assert_eq!(first_usage["usage"], serde_json::json!({"input_tokens":1_000,"output_tokens":100}));

	coordinator.continue_worker("agent", "Second").await.unwrap();

	let second = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let turn = second.active_turn_id.as_deref().unwrap();

	coordinator.handle_event(event(turn, 1_300, 150, 250)).await.unwrap();
	coordinator.handle_event(event(turn, 1_300, 150, 250)).await.unwrap();

	assert_eq!(
		coordinator.store.read_agent_turn_usage(thread.clone(), turn.into()).await.unwrap(),
		Some((300, 50))
	);

	let context: Value = serde_json::from_str(
		&coordinator.store.read_agent_usage("agent".into()).await.unwrap().unwrap(),
	)
	.unwrap();

	assert_eq!(context["context_tokens"], 250);

	complete(&mut coordinator, "agent").await;

	let events = coordinator.store.read_agent_work_events("agent".into(), 10).await.unwrap();
	let second_usage: Value = serde_json::from_str(&events.last().unwrap().payload).unwrap();

	assert_eq!(second_usage["usage"], serde_json::json!({"input_tokens":300,"output_tokens":50}));

	coordinator.continue_worker("agent", "Third").await.unwrap();

	let third = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let turn = third.active_turn_id.as_deref().unwrap();

	coordinator.handle_event(event(turn, 10, 5, 15)).await.unwrap();
	coordinator.handle_event(event(turn, 2_000, 200, 200)).await.unwrap();

	assert!(
		coordinator
			.store
			.read_agent_turn_usage(thread.clone(), turn.into())
			.await
			.unwrap()
			.is_none()
	);
}

#[tokio::test]
async fn native_usage_replay_restores_context_and_the_next_turn_baseline() {
	let (mut coordinator, _sent, _directory) = fixture_with_history(serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"opaque turn/1","status":"completed","items":[]}]}}})).await;
	let first = coordinator.start_agent("agent", "Initial").await.unwrap();

	complete(&mut coordinator, "agent").await;

	coordinator.loaded_threads.clear();
	coordinator.continue_worker("agent", "Next").await.unwrap();

	let current = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let thread = current.codex_thread_id.clone().unwrap();
	let event = |turn: &str, input: i64, output: i64, context: i64| ServerEvent::Notification {
		method: "thread/tokenUsage/updated".into(),
		params: serde_json::json!({"threadId":thread,"turnId":turn,"tokenUsage":{
			"total":{"inputTokens":input,"outputTokens":output},
			"last":{"totalTokens":context},"modelContextWindow":10_000}}),
	};

	coordinator
		.handle_event(event(first.active_turn_id.as_deref().unwrap(), 1_000, 100, 700))
		.await
		.unwrap();

	let restored: Value = serde_json::from_str(
		&coordinator.store.read_agent_usage("agent".into()).await.unwrap().unwrap(),
	)
	.unwrap();

	assert_eq!(restored["context_tokens"], 700);

	coordinator
		.handle_event(event(current.active_turn_id.as_deref().unwrap(), 1_300, 150, 450))
		.await
		.unwrap();
	coordinator
		.handle_event(event(first.active_turn_id.as_deref().unwrap(), 1_000, 100, 700))
		.await
		.unwrap();

	assert_eq!(
		coordinator
			.store
			.read_agent_turn_usage(thread, current.active_turn_id.unwrap())
			.await
			.unwrap(),
		Some((300, 50))
	);
}

#[tokio::test]
async fn configured_message_dispatches_native_images_skills_and_exact_turn_settings_once() {
	let (mut coordinator, mut sent, _directory) = fixture().await;

	AgentCoordinator::reserve_root(&coordinator.store, "agent", "Personal Agent").await.unwrap();

	coordinator.store.enqueue_agent_event(EnqueueAgentEvent {
        source_event_id:"configured-message".into(),work_item_id:"agent".into(),event_kind:"user_message".into(),
        payload:serde_json::json!({"text":"Inspect these files", "options":{
            "execution":{"model":"different-model","reasoning_effort":"high","fast":true},
            "attachments":[{"path":"/tmp/example.png","image":true},{"path":"/tmp/example.rs","image":false},{"path":"/tmp/skills (exact)/SKILL.md","image":false,"skill_name":"selected-skill"}]}}).to_string(),
    }).await.unwrap();
	coordinator.wake_pending().await.unwrap();
	coordinator.wake_pending().await.unwrap();

	let starts: Vec<_> =
		iter::from_fn(|| sent.try_recv().ok()).filter(|r| r["method"] == "turn/start").collect();

	assert_eq!(starts.len(), 1);

	let params = &starts[0]["params"];

	assert_eq!(params["model"], "different-model");
	assert_eq!(params["effort"], "high");
	assert_eq!(params["serviceTier"], "priority");
	assert_eq!(params["serviceTierForTurn"], "priority");
	assert_eq!(params["input"][0]["text"], "Inspect these files");
	assert_eq!(
		params["input"][1],
		serde_json::json!({"type":"localImage","path":"/tmp/example.png"})
	);
	assert!(params["input"][2]["text"].as_str().unwrap().contains("/tmp/example.rs"));
	assert_eq!(
		params["input"][3],
		serde_json::json!({"type":"skill","name":"selected-skill","path":"/tmp/skills (exact)/SKILL.md"})
	);

	let mut params = serde_json::json!({"input":[],"serviceTier":"priority"});

	agent::apply_message_options(&mut params,&serde_json::json!({"options":{"execution":{"model":"selected-model","reasoning_effort":"medium","fast":false},"attachments":[]}}).to_string()).unwrap();

	assert!(params["serviceTier"].is_null());
	assert_eq!(params["serviceTierForTurn"], "default");

	let tiered = serde_json::json!({"options":{"execution":{"model":"chosen","reasoning_effort":"high","fast":false,"service_tier":"ultrafast"},"attachments":[]}});

	agent::apply_message_options(&mut params, &tiered.to_string()).unwrap();

	assert_eq!(params["serviceTier"], "ultrafast");
	assert_eq!(params["serviceTierForTurn"], "ultrafast");
}

#[tokio::test]
async fn steer_uses_exact_running_turn_and_never_starts_a_second_turn() {
	let (mut agent, mut sent, _dir) = fixture().await;

	agent.start_agent("agent", "Initial task").await.unwrap();

	let work = agent.store.get_agent_work_item("agent".into()).await.unwrap();
	let turn = work.active_turn_id.unwrap();

	while sent.try_recv().is_ok() {}

	assert!(agent.steer_work("agent", "stale", "stale-command", "Supplement", &[]).await.is_err());
	assert!(sent.try_recv().is_err());

	agent.steer_work("agent", &turn, "steer-command", "Supplement", &[]).await.unwrap();

	let request = sent.recv().await.unwrap();

	assert_eq!(request["method"], "turn/steer");
	assert_eq!(request["params"]["expectedTurnId"], turn);
	assert_eq!(request["params"]["input"][0]["text"], "Supplement");
	assert_eq!(request["params"]["threadId"], work.codex_thread_id.unwrap());
	assert_eq!(request["params"]["clientUserMessageId"], "steer-command");
	assert!(agent.steer_work("agent", &turn, "steer-command", "Supplement", &[]).await.is_err());
	assert!(sent.try_recv().is_err());

	let receipts = agent.store.list_agent_events_for_turn(turn.clone(), 100).await.unwrap();

	assert!(
		receipts.iter().any(|e| e.event_kind == "user_message" && e.payload.contains("Supplement"))
	);
	assert!(agent.interrupt_work("agent", "stale").await.is_err());

	agent.interrupt_work("agent", &turn).await.unwrap();

	assert_eq!(sent.recv().await.unwrap()["method"], "turn/interrupt");

	complete(&mut agent, "agent").await;

	agent.wake_pending().await.unwrap();

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start");
	}
}

#[tokio::test]
async fn rejected_or_uncertain_steer_never_replays_as_queued_input() {
	for flags in
		[serde_json::json!({"_steer_error":true}), serde_json::json!({"_steer_disconnect":true})]
	{
		let (mut agent, mut sent, _dir) = fixture_with_history(flags).await;

		agent.start_agent("agent", "Initial task").await.unwrap();

		let turn =
			agent.store.get_agent_work_item("agent".into()).await.unwrap().active_turn_id.unwrap();

		while sent.try_recv().is_ok() {}

		assert!(agent.steer_work("agent", &turn, "one-attempt", "Supplement", &[]).await.is_err());
		assert!(agent.store.list_undelivered_agent_events(100).await.unwrap().is_empty());
		assert!(agent.steer_work("agent", &turn, "one-attempt", "Supplement", &[]).await.is_err());
		assert_eq!(sent.recv().await.unwrap()["method"], "turn/steer");
		assert!(sent.try_recv().is_err());
	}
}

#[tokio::test]
async fn native_activity_notifications_reach_history_without_agent_delivery() {
	let (mut coordinator, _sent, _directory) = fixture().await;
	let work = coordinator.start_agent("agent", "Talk").await.unwrap();

	for method in ["item/started", "item/completed"] {
		coordinator
			.handle_event(ServerEvent::Notification {
				method: method.into(),
				params: serde_json::json!({"threadId":work.codex_thread_id,"turnId":work.active_turn_id,
				"item":{"id":"compact", "type":"contextCompaction"}}),
			})
			.await
			.unwrap();
	}

	let (events, _) =
		coordinator.store.read_agent_transcript("agent".into(), None, 32).await.unwrap();
	let receipts: Vec<_> =
		events.iter().filter(|event| event.event_kind.starts_with("activity_")).collect();

	assert_eq!(receipts.len(), 1);

	let activity: AgentActivityDto = serde_json::from_str(&receipts[0].payload).unwrap();

	assert_eq!(activity.label, "Compacting context");
	assert_eq!(activity.status, "completed");
	assert_eq!(receipts[0].disposition, Some(AgentDisposition::Resolved));
}

#[tokio::test]
async fn native_revert_retires_exact_thread_requests_without_replies_or_replay() {
	let (mut agent, mut sent, directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();
	agent.handle_event(ServerEvent::Notification {
		method: "item/agentMessage/delta".into(),
		params: serde_json::json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","itemId":"partial","delta":"Removed native output"}),
	}).await.unwrap();

	assert_eq!(agent.store.read_agent_output("agent".into()).await.unwrap().len(), 1);

	agent
		.store
		.invalidate_agent_output("opaque thread/1".into(), Some("unbound-generation".into()))
		.await
		.unwrap();

	assert_eq!(agent.store.read_agent_output("agent".into()).await.unwrap().len(), 1);

	while sent.try_recv().is_ok() {}

	let id = RequestId::Number(73);

	agent.handle_event(ServerEvent::Request { id: id.clone(), method: "item/commandExecution/requestApproval".into(), params: serde_json::json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","itemId":"item","command":"pwd"}) }).await.unwrap();

	let event_id = agent.pending_requests[&id];

	agent
		.handle_event(ServerEvent::Notification {
			method: "thread/reverted".into(),
			params: serde_json::json!({"threadId":"other"}),
		})
		.await
		.unwrap();

	assert!(agent.pending_requests.contains_key(&id));
	assert_eq!(agent.store.read_agent_output("agent".into()).await.unwrap().len(), 1);

	for _ in 0..2 {
		agent
			.handle_event(ServerEvent::Notification {
				method: "thread/reverted".into(),
				params: serde_json::json!({"threadId":"opaque thread/1"}),
			})
			.await
			.unwrap();
	}

	assert!(!agent.pending_requests.contains_key(&id));
	assert!(agent.store.read_agent_output("agent".into()).await.unwrap().is_empty());

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
	let reopened = SqliteStore::open(&root.paths()).unwrap();

	assert!(reopened.read_agent_output("agent".into()).await.unwrap().is_empty());

	let event = agent.store.get_agent_inbox_event(event_id).await.unwrap();

	assert_eq!(event.disposition, Some(AgentDisposition::Resolved));
	assert!(
		agent
			.respond_pending_event(event_id, serde_json::json!({"decision":"accept"}))
			.await
			.is_err()
	);
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn native_request_resolution_requires_exact_thread_and_request_identity() {
	let (mut coordinator, mut sent, _directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	for id in [RequestId::String("shared-item-A".into()), RequestId::Number(7)] {
		coordinator.handle_event(ServerEvent::Request { id: id.clone(), method:"item/commandExecution/requestApproval".into(),params:serde_json::json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","itemId":"shared-item","command":"pwd"}) }).await.unwrap();
	}

	let id = RequestId::String("shared-item-A".into());
	let event_id = coordinator.pending_requests[&id];

	for params in [
		serde_json::json!({"threadId":"other-thread","requestId":id}),
		serde_json::json!({"threadId":"opaque thread/1","requestId":"7"}),
		serde_json::json!({"threadId":"opaque thread/1","requestId":null}),
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
				params: serde_json::json!({"threadId":"opaque thread/1","requestId":id}),
			})
			.await
			.unwrap();
	}

	assert!(!coordinator.pending_requests.contains_key(&id));
	assert!(coordinator.pending_requests.contains_key(&RequestId::Number(7)));

	let event = coordinator.store.get_agent_inbox_event(event_id).await.unwrap();

	assert_eq!(event.disposition, Some(AgentDisposition::Resolved));
	assert!(event.disposition_note.unwrap().contains("no local response was sent"));
	assert!(
		coordinator
			.respond_pending_event(event_id, serde_json::json!({"decision":"accept"}))
			.await
			.is_err()
	);
	assert!(sent.try_recv().is_err());
	assert_eq!(
		coordinator.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Running
	);
}

#[tokio::test]
async fn async_question_answers_survive_replay_and_reopening_without_waking_work() {
	let (mut coordinator, mut sent, directory) = fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let first_id = decodex_protocol::agent_async_question_id("questions", 0);
	let reply = decodex_protocol::agent_async_question_reply(
		&AgentAsyncQuestionDto {
			arrived_live: false,
			id: first_id.clone(),
			title: "Same".into(),
			options: vec![],
		},
		"A",
	)
	.unwrap();
	let response = serde_json::json!({"type":"userMessage","id":"answer","content":[{"type":"text","text":reply.as_str()}]});
	let questions = serde_json::json!({"type":"agentMessage","delivery":"async","id":"questions","text":"Choose","questions":[{"title":"Same","options":["A","B"]},{"title":"Same","options":["A","B"]}]});

	// A committed answer can arrive before the corresponding history item.
	for item in [response, questions.clone(), questions.clone()] {
		coordinator
			.handle_event(ServerEvent::Notification {
				method: "item/completed".into(),
				params: serde_json::json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","item":item}),
			})
			.await
			.unwrap();
	}

	let pending = coordinator.store.read_agent_async_questions("agent".into()).await.unwrap();

	assert_eq!(pending.len(), 1);
	assert_eq!(pending[0].question_id, decodex_protocol::agent_async_question_id("questions", 1));

	coordinator
		.store
		.resolve_agent_async_questions("unowned".into(), vec![pending[0].question_id.clone()])
		.await
		.unwrap();

	assert_eq!(
		coordinator.store.read_agent_async_questions("agent".into()).await.unwrap().len(),
		1
	);

	let paths =
		DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap().paths();
	let reopened = SqliteStore::open(&paths).unwrap();

	assert_eq!(reopened.read_agent_async_questions("agent".into()).await.unwrap().len(), 1);

	// Older desktop replies identify the source message and resolve all its questions.
	reopened
		.resolve_agent_async_questions("opaque thread/1".into(), vec!["questions".into()])
		.await
		.unwrap();
	coordinator
		.observe_async_question_item("opaque thread/1", "opaque turn/1", &questions)
		.await
		.unwrap();

	assert!(reopened.read_agent_async_questions("agent".into()).await.unwrap().is_empty());
	assert!(coordinator.store.list_pending_agent_events(100).await.unwrap().is_empty());
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn async_question_upgrade_reads_native_history_and_preserves_later_questions() {
	let question = |id: &str| serde_json::json!({"id":id,"type":"agentMessage","delivery":"async","text":"Question","questions":[{"title":"Which?","options":["A","B"]}]});
	let answer = decodex_protocol::agent_async_question_reply(
		&AgentAsyncQuestionDto {
			arrived_live: false,
			id: decodex_protocol::agent_async_question_id("answered", 0),
			title: "Which?".into(),
			options: vec![],
		},
		"B",
	)
	.unwrap();
	let history = serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"old","status":"completed","items":[question("old"),{"type":"userMessage","content":[{"type":"text","text":"New task"}]},question("answered"),{"type":"userMessage","content":[{"type":"text","text":answer.as_str()}]},question("pending")]}]}}});
	let (mut agent, mut sent, directory) = fixture_with_history(history).await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
	let db = Connection::open(root.paths().product_database_file()).unwrap();

	db.execute(
		"INSERT INTO agent_async_recovery(work_id,thread_id) VALUES('agent','opaque thread/1')",
		[],
	)
	.unwrap();

	drop(db);

	agent.recover_async_questions().await.unwrap();

	let pending = agent.store.read_agent_async_questions("agent".into()).await.unwrap();

	assert_eq!(pending.len(), 1);
	assert_eq!(pending[0].item_id, "pending");
	assert!(agent.store.pending_agent_async_recovery().await.unwrap().is_empty());

	while let Ok(request) = sent.try_recv() {
		assert_eq!(request["method"], "thread/read");
	}

	agent.recover_async_questions().await.unwrap();

	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn reverted_async_history_reopens_retained_questions_and_removes_deleted_questions() {
	let item = serde_json::json!({"id":"question","type":"agentMessage","delivery":"async","text":"Question","questions":[{"title":"Which?"}]});
	let history = serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"old","status":"completed","items":[item.clone()]}]}}});
	let (mut agent, mut sent, _directory) = fixture_with_history(history).await;

	agent.start_agent("agent", "Coordinate").await.unwrap();
	agent.observe_async_question_item("opaque thread/1", "old", &item).await.unwrap();

	let id = decodex_protocol::agent_async_question_id("question", 0);

	agent
		.store
		.resolve_agent_async_questions("opaque thread/1".into(), vec![id.clone()])
		.await
		.unwrap();

	assert!(agent.store.read_agent_async_questions("agent".into()).await.unwrap().is_empty());

	while sent.try_recv().is_ok() {}

	agent
		.handle_event(ServerEvent::Notification {
			method: "thread/reverted".into(),
			params: serde_json::json!({"threadId":"opaque thread/1"}),
		})
		.await
		.unwrap();

	assert!(agent.store.agent_async_questions_recovering("agent".into()).await.unwrap());

	agent.recover_async_questions().await.unwrap();

	let questions = agent.store.read_agent_async_questions("agent".into()).await.unwrap();

	assert_eq!(questions.len(), 1);
	assert_eq!(questions[0].question_id, id);

	while let Ok(request) = sent.try_recv() {
		assert_eq!(request["method"], "thread/read");
	}

	let queued = agent
		.store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "offline-answer".into(),
			work_item_id: "agent".into(),
			event_kind: "async_question_answer".into(),
			payload: serde_json::json!({"text":"answer","asyncQuestionId":id}).to_string(),
		})
		.await
		.unwrap();
	let empty =
		serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[]}}});
	let (mut reconnected, mut sent, _other) = fixture_with_history(empty).await;

	reconnected.store = agent.store.clone();

	reconnected.store.queue_agent_async_reconnection().await.unwrap();
	reconnected.recover_async_questions().await.unwrap();

	assert!(reconnected.store.read_agent_async_questions("agent".into()).await.unwrap().is_empty());
	assert!(!reconnected.store.agent_async_questions_recovering("agent".into()).await.unwrap());
	assert_eq!(
		reconnected.store.get_agent_inbox_event(queued.id).await.unwrap().disposition,
		Some(AgentDisposition::Resolved)
	);

	while let Ok(request) = sent.try_recv() {
		assert_eq!(request["method"], "thread/read");
	}
}

#[tokio::test]
async fn other_client_input_blocks_question_writes_before_owner_observation() {
	for ordinary in [false, true] {
		let (mut agent, mut sent, _directory) = fixture().await;

		agent.start_agent("agent", "Coordinate").await.unwrap();

		let item = serde_json::json!({"id":"questions","type":"agentMessage","delivery":"async","questions":[{"title":"First?"},{"title":"Second?"}]});

		agent.observe_async_question_item("opaque thread/1", "opaque turn/1", &item).await.unwrap();

		let questions = decodex_protocol::project_agent_async_questions(&item).unwrap();
		let text = if ordinary {
			"New task".to_owned()
		} else {
			decodex_protocol::agent_async_question_reply(&questions[0], "A")
				.unwrap()
				.as_str()
				.to_owned()
		};

		while sent.try_recv().is_ok() {}

		let (incoming, frames) = mpsc::channel(8);
		let (outgoing, mut writes) = mpsc::channel(8);
		let (client, mut events) = AppServerClient::from_framed(1, frames, outgoing).unwrap();

		agent.client = client.clone();

		incoming.send(Ok(serde_json::json!({"method":"item/completed","params":{"threadId":"opaque thread/1","turnId":"opaque turn/1","item":{"id":"input","type":"userMessage","content":[{"type":"text","text":text}]}}}))).await.unwrap();

		time::timeout(Duration::from_secs(2), async {
			while client.question_revision() == 0 {
				task::yield_now().await;
			}
		})
		.await
		.unwrap();

		assert!(
			agent
				.answer_async_question("agent", &questions[0].id, "B", "old-answer")
				.await
				.is_err()
		);
		assert!(writes.try_recv().is_err());
		assert!(sent.try_recv().is_err());
		assert_eq!(client.history_revision(), 0);

		if !ordinary {
			agent.handle_event(events.recv().await.unwrap()).await.unwrap();

			let pending = agent.store.read_agent_async_questions("agent".into()).await.unwrap();

			assert_eq!(pending.len(), 1);
			assert_eq!(pending[0].question_id, questions[1].id);
			assert!(client.question_guard(agent.handled_question_revision).is_some());
		}
	}
}

#[tokio::test]
async fn transport_revert_blocks_old_question_before_coordinator_reads_notification() {
	let (mut agent, mut sent, _directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	let question = serde_json::json!({"id":"question","type":"agentMessage","delivery":"async","questions":[{"title":"Which?"}]});

	agent.observe_async_question_item("opaque thread/1", "opaque turn/1", &question).await.unwrap();

	while sent.try_recv().is_ok() {}

	let (incoming, frames) = mpsc::channel(8);
	let (outgoing, mut writes) = mpsc::channel(8);
	let (client, mut notifications) = AppServerClient::from_framed(1, frames, outgoing).unwrap();

	agent.client = client.clone();

	incoming
		.send(Ok(
			serde_json::json!({"method":"thread/reverted","params":{"threadId":"opaque thread/1"}}),
		))
		.await
		.unwrap();

	time::timeout(Duration::from_secs(2), async {
		while client.history_revision() == 0 {
			task::yield_now().await;
		}
	})
	.await
	.unwrap();

	let id = decodex_protocol::agent_async_question_id("question", 0);

	assert!(matches!(
		agent.answer_async_question("agent", &id, "A", "stale-ui").await,
		Err(super::AgentError::Invalid(_))
	));
	assert!(writes.try_recv().is_err());
	assert!(sent.try_recv().is_err());
	assert_eq!(
		agent.store.read_agent_async_questions("agent".into()).await.unwrap().len(),
		1,
		"old projection exists until owner handles the notification"
	);

	agent.handle_event(notifications.recv().await.unwrap()).await.unwrap();

	assert!(agent.store.agent_async_questions_recovering("agent".into()).await.unwrap());
	assert!(agent.answer_async_question("agent", &id, "A", "after-owner").await.is_err());
	assert!(writes.try_recv().is_err());
}

#[tokio::test]
async fn stale_history_guard_prevents_async_turn_and_steer_without_unknown_receipts() {
	for running in [false, true] {
		let (mut agent, mut sent, directory) = fixture().await;

		agent.start_agent("agent", "Coordinate").await.unwrap();

		if !running {
			agent.handle_event(ServerEvent::Notification {method:"turn/completed".into(),params:serde_json::json!({"threadId":"opaque thread/1","turn":{"id":"opaque turn/1","status":"completed","items":[]}})}).await.unwrap();
		}
		if !running {
			let root =
				DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
			let db = Connection::open(root.paths().product_database_file()).unwrap();

			db.execute("UPDATE agent_work_items SET status='wait',next_check_at_micros=9999999999999999 WHERE id='agent'",[]).unwrap();
		}

		let work = agent.store.get_agent_work_item("agent".into()).await.unwrap();
		let (foreign, _events, _process) = {
			let (io, remote) = io::duplex(4_096);
			let (read, write) = io::split(io);
			let (client, events) = AppServerClient::from_io(read, write);

			(client, events, remote)
		};
		let guard = foreign.history_guard(0).unwrap();

		while sent.try_recv().is_ok() {}

		let result = if running {
			agent
				.steer_work_with_question_reply(
					"agent",
					"opaque turn/1",
					"stale-answer",
					"answer",
					AgentInputExtras { attachments: &[], task_references: &[] },
					Some(("question", guard)),
				)
				.await
				.map(|_| String::new())
		} else {
			let event = agent
				.store
				.enqueue_agent_event(EnqueueAgentEvent {
					source_event_id: "stale-answer".into(),
					work_item_id: "agent".into(),
					event_kind: "async_question_answer".into(),
					payload: serde_json::json!({"text":"answer","asyncQuestionId":"question"})
						.to_string(),
				})
				.await
				.unwrap();

			agent.dispatch_with_claim(&work, "answer", vec![event.id], None, Some(guard)).await
		};

		if running {
			assert!(matches!(
				result,
				Err(super::AgentError::InputNotSent(
					decodex_database::AgentDispatchRefusal::SettingsChanged
				))
			));
		} else {
			// The foreign guard is rejected before a turn dispatch claim exists.
			assert!(matches!(result, Err(super::AgentError::Transport(ClientError::StaleHistory))));
		}

		let after = agent.store.get_agent_work_item("agent".into()).await.unwrap();

		assert_eq!(after.dispatch_state, work.dispatch_state);
		assert_eq!(after.status, work.status);
		assert_eq!(after.next_check_at_micros, work.next_check_at_micros);
		assert!(
			!agent
				.store
				.agent_async_answer_pending("agent".into(), "question".into())
				.await
				.unwrap()
		);
		assert!(sent.try_recv().is_err());
	}
}

#[tokio::test]
async fn async_revert_marker_survives_reopen_and_preserves_uncertain_deliveries() {
	let (mut agent, mut sent, directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let mut ids = Vec::new();

	for kind in ["unsent", "unknown", "accepted", "ordinary"] {
		let event = agent
			.store
			.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id: format!("revert-{kind}"),
				work_item_id: "agent".into(),
				event_kind: if kind == "ordinary" {
					"user_message"
				} else {
					"async_question_answer"
				}
				.into(),
				payload: serde_json::json!({"text":"answer","asyncQuestionId":"q"}).to_string(),
			})
			.await
			.unwrap();

		ids.push(event.id);
	}

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
	let db = Connection::open(root.paths().product_database_file()).unwrap();

	db.execute(
		"UPDATE agent_inbox_events SET delivered_turn_id='',delivery_work_item_id='agent' WHERE id=?1",
		[ids[1]],
	)
	.unwrap();
	db.execute(
		"UPDATE agent_inbox_events SET delivered_turn_id='accepted-turn',delivery_work_item_id='agent' WHERE id=?1",
		[ids[2]],
	)
	.unwrap();

	drop(db);

	agent.store.queue_agent_async_revert("unrelated".into()).await.unwrap();

	assert!(agent.store.get_agent_inbox_event(ids[0]).await.unwrap().disposition.is_none());

	agent.store.queue_agent_async_revert("opaque thread/1".into()).await.unwrap();

	let reopened = SqliteStore::open(&root.paths()).unwrap();

	assert!(reopened.agent_async_questions_recovering("agent".into()).await.unwrap());
	assert_eq!(
		reopened.get_agent_inbox_event(ids[0]).await.unwrap().disposition,
		Some(AgentDisposition::Resolved)
	);

	for id in &ids[1..] {
		assert!(reopened.get_agent_inbox_event(*id).await.unwrap().disposition.is_none());
	}

	assert_eq!(
		reopened.get_agent_inbox_event(ids[1]).await.unwrap().delivered_turn_id.as_deref(),
		Some("")
	);
	assert!(reopened.agent_async_answer_pending("agent".into(), "q".into()).await.unwrap());

	reopened
		.request_agent_async_recovery("opaque thread/1".into(), "new-prompt".into())
		.await
		.unwrap();

	assert!(
		!reopened
			.replace_agent_async_projection(
				"agent".into(),
				"opaque thread/1".into(),
				None,
				vec![],
				vec![]
			)
			.await
			.unwrap()
	);
	assert!(reopened.agent_async_questions_recovering("agent".into()).await.unwrap());
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn incomplete_async_recovery_hides_cards_until_a_later_complete_read() {
	let bad = serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","historyMode":"unknown"}}});
	let (mut agent, mut sent, directory) = fixture_with_history(bad).await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	let item = serde_json::json!({"id":"pending","type":"agentMessage","delivery":"async","text":"Question","questions":[{"title":"Which?"}]});

	agent.observe_async_question_item("opaque thread/1", "old", &item).await.unwrap();

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
	let db = Connection::open(root.paths().product_database_file()).unwrap();

	db.execute(
		"INSERT INTO agent_async_recovery(work_id,thread_id) VALUES('agent','opaque thread/1')",
		[],
	)
	.unwrap();

	drop(db);

	while sent.try_recv().is_ok() {}

	agent.recover_async_questions().await.unwrap();

	assert!(agent.store.agent_async_questions_recovering("agent".into()).await.unwrap());
	assert!(agent.store.read_agent_async_questions("agent".into()).await.unwrap().is_empty());

	while let Ok(request) = sent.try_recv() {
		assert_eq!(request["method"], "thread/read");
	}

	let good = serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","historyMode":"paginated","turns":[{"id":"old","status":"completed","items":[item]}]}}});
	let (mut recovered, mut sent, _other_directory) = fixture_with_history(good).await;

	recovered.store = agent.store.clone();

	recovered.recover_async_questions().await.unwrap();

	assert!(!recovered.store.agent_async_questions_recovering("agent".into()).await.unwrap());
	assert_eq!(recovered.store.read_agent_async_questions("agent".into()).await.unwrap().len(), 1);

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
		let (mut agent, mut sent, _directory) = fixture().await;

		agent.start_agent("agent", "Coordinate").await.unwrap();

		let worker = agent.create_worker("agent", "worker", "Inspect").await.unwrap();
		let thread = worker.codex_thread_id.unwrap();
		let turn = worker.active_turn_id.unwrap();
		let item = serde_json::json!({"id":"question","type":"agentMessage","delivery":"async","text":"Questions","questions":[{"title":"Same title"},{"title":"Same title"}]});

		agent.observe_async_question_item(&thread, &turn, &item).await.unwrap();

		if idle {
			agent.store.complete_agent_turn("worker".into(), turn.clone()).await.unwrap();
		}

		while sent.try_recv().is_ok() {}

		let question_id = decodex_protocol::agent_async_question_id("question", 1);

		agent
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
		assert_eq!(
			inputs[0]["params"].get("turnTrigger").and_then(Value::as_str),
			idle.then_some("user")
		);
		assert_eq!(inputs[0]["params"]["threadId"], thread);

		if !idle {
			assert_eq!(inputs[0]["params"]["expectedTurnId"], turn);
		}

		let replies = decodex_protocol::parse_agent_async_question_replies(
			inputs[0]["params"]["input"][0]["text"].as_str().unwrap(),
		)
		.unwrap();

		assert_eq!(replies[0].question_item_id, question_id);
		assert_eq!(replies[0].answer, "Explicit choice");

		let pending = agent.store.read_agent_async_questions("worker".into()).await.unwrap();

		assert_eq!(pending.len(), 1);
		assert_eq!(
			pending[0].question_id,
			decodex_protocol::agent_async_question_id("question", 0)
		);
		assert!(
			agent
				.answer_async_question("worker", &question_id, "Explicit choice", "answer-1")
				.await
				.is_err()
		);

		agent.wake_pending().await.unwrap();

		assert!(sent.try_recv().is_err());
	}
}

#[tokio::test]
async fn rejected_or_uncertain_async_answers_keep_question_and_do_not_queue_retry() {
	for history in
		[serde_json::json!({"_steer_error":true}), serde_json::json!({"_steer_disconnect":true})]
	{
		let uncertain = history["_steer_disconnect"] == true;
		let (mut agent, mut sent, directory) = fixture_with_history(history).await;

		agent.start_agent("agent", "Coordinate").await.unwrap();
		agent.observe_async_question_item("opaque thread/1","opaque turn/1",&serde_json::json!({"id":"question","type":"agentMessage","delivery":"async","questions":[{"title":"Which?"}]})).await.unwrap();

		while sent.try_recv().is_ok() {}

		let id = decodex_protocol::agent_async_question_id("question", 0);

		assert!(agent.answer_async_question("agent", &id, "A", "once").await.is_err());
		assert_eq!(agent.store.read_agent_async_questions("agent".into()).await.unwrap().len(), 1);

		agent.wake_pending().await.unwrap();

		let mut count = 0;

		while let Ok(request) = sent.try_recv() {
			assert_eq!(request["method"], "turn/steer");

			count += 1;
		}

		assert_eq!(count, 1);
		assert!(agent.store.list_undelivered_agent_events(100).await.unwrap().is_empty());
		assert_eq!(
			agent.store.agent_async_answer_pending("agent".into(), id.clone()).await.unwrap(),
			uncertain
		);

		if uncertain {
			let root =
				DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
			let (mut reopened, mut reopened_sent, _other) = fixture().await;

			reopened.store = SqliteStore::open(&root.paths()).unwrap();

			assert!(
				reopened
					.answer_async_question("agent", &id, "A", "new-command-after-restart")
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
		let question = |id: &str| serde_json::json!({"id":id,"type":"agentMessage","delivery":"async","questions":[{"title":"Question"}]});
		let prompt = serde_json::json!({"id":"remote-prompt","type":"userMessage","content":[{"type":"text","text":"Move on"}]});
		let mut items = vec![question("old")];

		if contains_prompt {
			items.push(prompt.clone());
		}

		items.push(question("new"));

		let history = serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"opaque turn/1","status":"inProgress","items":items}]}}});
		let (mut agent, mut sent, _directory) = fixture_with_history(history).await;

		agent.start_agent("agent", "Coordinate").await.unwrap();

		while sent.try_recv().is_ok() {}

		for _ in 0..2 {
			agent
				.observe_notification(
					"item/completed",
					&serde_json::json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","item":prompt}),
				)
				.await
				.unwrap();

			assert_eq!(
				agent.store.agent_async_questions_recovering("agent".into()).await.unwrap(),
				!contains_prompt
			);

			let questions = agent.store.read_agent_async_questions("agent".into()).await.unwrap();

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
	let (mut agent, mut sent, directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();
	agent.observe_async_question_item("opaque thread/1","opaque turn/1",&serde_json::json!({"id":"question","type":"agentMessage","delivery":"async","questions":[{"title":"Which?"}]})).await.unwrap();
	agent.store.complete_agent_turn("agent".into(), "opaque turn/1".into()).await.unwrap();

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
	let db = Connection::open(root.paths().product_database_file()).unwrap();

	db.execute("DELETE FROM agent_tool_versions WHERE work_id='agent'", []).unwrap();

	drop(db);

	while sent.try_recv().is_ok() {}

	agent
		.answer_async_question(
			"agent",
			&decodex_protocol::agent_async_question_id("question", 0),
			"A",
			"answer-old-manager",
		)
		.await
		.unwrap();

	let mut started = false;

	while let Ok(request) = sent.try_recv() {
		assert!(
			["thread/resume", "thread/read", "thread/inject_items", "turn/start"]
				.contains(&request["method"].as_str().unwrap())
		);
		assert_eq!(request["params"]["threadId"], "opaque thread/1");

		started |= request["method"] == "turn/start";
	}

	assert!(started);
	assert_eq!(agent.store.agent_tool_version("agent".into()).await.unwrap(), 1);
}

#[tokio::test]
async fn misalignment_precaution_survives_reopen_and_blocks_ordinary_dispatch() {
	let (mut agent, mut sent, directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	agent.enqueue_user_message("agent", "queued-before-stop", "Queued work").await.unwrap();

	let error = serde_json::json!({"codexErrorInfo":"misalignmentPolicyViolation","misalignment":{"detailedExplanation":"Review the scope.","steer":{"message":"Continue with the clarified scope"}}});

	agent
		.observe_notification(
			"error",
			&serde_json::json!({"threadId":"opaque thread/1","turnId":"stale","willRetry":false,"error":error}),
		)
		.await
		.unwrap();

	assert!(agent.store.agent_misalignment("agent".into()).await.unwrap().is_none());

	agent
		.observe_notification(
			"error",
			&serde_json::json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","willRetry":true,"error":error}),
		)
		.await
		.unwrap();

	assert!(agent.store.agent_misalignment("agent".into()).await.unwrap().is_none());

	agent
		.observe_notification(
			"error",
			&serde_json::json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","willRetry":false,"error":error}),
		)
		.await
		.unwrap();
	agent
		.observe_misalignment(
			"opaque thread/1",
			"opaque turn/1",
			&serde_json::json!({"codexErrorInfo":"misalignmentPolicyViolation"}),
		)
		.await
		.unwrap();

	let saved = agent.store.agent_misalignment("agent".into()).await.unwrap().unwrap();

	assert!(agent.enqueue_user_message("agent", "after-stop", "New work").await.is_err());
	assert!(agent.store.list_undelivered_agent_events(100).await.unwrap().is_empty());
	assert!(saved.details_json.as_deref().unwrap().contains("Review the scope."));
	assert!(agent.steer_work("agent", "opaque turn/1", "ordinary", "Continue", &[]).await.is_err());

	agent.store.complete_agent_turn("agent".into(), "opaque turn/1".into()).await.unwrap();

	assert!(agent.continue_worker("agent", "Continue").await.is_err());
	assert!(sent.try_recv().is_err());

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
	let reopened = SqliteStore::open(&root.paths()).unwrap();

	assert_eq!(reopened.agent_misalignment("agent".into()).await.unwrap(), Some(saved));
}

#[tokio::test]
async fn explicit_misalignment_continuation_uses_native_override_and_clears_after_ack() {
	let error = serde_json::json!({"codexErrorInfo":"misalignmentPolicyViolation","misalignment":{"detailedExplanation":"Review scope","steer":{"message":"Clarified scope"}}});
	let history = serde_json::json!({"_live_misalignment":error,"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"opaque turn/1","status":"failed","error":{"codexErrorInfo":"misalignmentPolicyViolation"},"items":[]}]}}});
	let (mut agent, mut sent, _directory) = fixture_with_history(history).await;

	agent.start_agent("agent", "Coordinate").await.unwrap();
	agent.observe_misalignment("opaque thread/1", "opaque turn/1", &error).await.unwrap();
	agent.store.complete_agent_turn("agent".into(), "opaque turn/1".into()).await.unwrap();

	let review = agent.store.agent_misalignment("agent".into()).await.unwrap().unwrap();

	while sent.try_recv().is_ok() {}

	let token = live_review_token(&agent, &review);

	assert!(
		agent
			.continue_misalignment("agent", review.clone(), "stale-click", "older-live-evidence")
			.await
			.is_err()
	);
	assert!(sent.try_recv().is_err());

	agent.continue_misalignment("agent", review, "acknowledged", &token).await.unwrap();

	let mut turns = Vec::new();

	while let Ok(request) = sent.try_recv() {
		if request["method"] == "turn/start" {
			turns.push(request);
		}
	}

	assert_eq!(turns.len(), 1);
	assert_eq!(turns[0]["params"]["threadId"], "opaque thread/1");
	assert_eq!(turns[0]["params"]["input"][0]["text"], "Clarified scope");
	assert!(turns[0]["params"].get("model").is_none());
	assert!(turns[0]["params"].get("effort").is_none());

	let metadata: Value = serde_json::from_str(
		turns[0]["params"]["responsesapiClientMetadata"]["misalignment_override"].as_str().unwrap(),
	)
	.unwrap();

	assert!(metadata["timestamp"].as_u64().unwrap() > 0);
	assert!(agent.store.agent_misalignment("agent".into()).await.unwrap().is_none());
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().active_turn_id.as_deref(),
		Some("opaque turn/2")
	);
}

#[tokio::test]
async fn misalignment_stale_rejected_and_uncertain_continuations_keep_precaution() {
	for outcome in ["changed", "rejected", "uncertain", "reverted"] {
		let error = serde_json::json!({"codexErrorInfo":"misalignmentPolicyViolation","misalignment":{"detailedExplanation":"Review scope","steer":{"message":"Clarified scope"}}});
		let mut native_error = error.clone();

		if outcome == "changed" {
			native_error["misalignment"]["detailedExplanation"] = serde_json::json!("New findings");
		}

		let history = serde_json::json!({"_live_misalignment":error,"_misalignment_revert_on_read":outcome=="reverted","_continuation_disconnect":outcome=="uncertain","_continuation_reject":outcome=="rejected","opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"opaque turn/1","status":"failed","error":native_error,"items":[]}]}}});
		let (mut agent, mut sent, directory) = fixture_with_history(history).await;

		agent.start_agent("agent", "Coordinate").await.unwrap();
		agent.observe_misalignment("opaque thread/1", "opaque turn/1", &error).await.unwrap();
		agent.store.complete_agent_turn("agent".into(), "opaque turn/1".into()).await.unwrap();

		let review = agent.store.agent_misalignment("agent".into()).await.unwrap().unwrap();

		while sent.try_recv().is_ok() {}

		let token = live_review_token(&agent, &review);
		let failure = agent
			.continue_misalignment("agent", review.clone(), "acknowledged", &token)
			.await
			.unwrap_err();

		assert_eq!(matches!(failure, AgentError::Rejected(_)), outcome != "uncertain");
		assert!(!failure.to_string().contains("private-steer-sentinel"));
		assert!(agent.store.agent_misalignment("agent".into()).await.unwrap().is_some());

		let state = agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state;

		assert_eq!(
			state,
			if outcome == "uncertain" {
				decodex_database::AgentDispatchState::Dispatching
			} else {
				decodex_database::AgentDispatchState::Idle
			}
		);

		let mut starts = 0;

		while let Ok(request) = sent.try_recv() {
			starts += usize::from(request["method"] == "turn/start");
		}

		assert_eq!(starts, usize::from(!["changed", "reverted"].contains(&outcome)));

		if outcome == "uncertain" {
			let root =
				DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
			let (mut reopened, mut requests, _other) = fixture().await;

			reopened.store = SqliteStore::open(&root.paths()).unwrap();

			assert!(
				reopened.continue_misalignment("agent", review, "new-key", &token).await.is_err()
			);
			assert!(requests.try_recv().is_err());
		}
	}
}

#[tokio::test]
async fn misalignment_saved_details_cannot_authorize_a_reconnected_transport() {
	let error = serde_json::json!({"codexErrorInfo":"misalignmentPolicyViolation","misalignment":{"detailedExplanation":"Review scope","steer":{"message":"Clarified scope"}}});
	let (mut agent, _sent, directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();
	agent.observe_misalignment("opaque thread/1", "opaque turn/1", &error).await.unwrap();
	agent.store.complete_agent_turn("agent".into(), "opaque turn/1".into()).await.unwrap();

	let review = agent.store.agent_misalignment("agent".into()).await.unwrap().unwrap();
	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();

	drop(agent);

	let (mut reopened, mut requests, _other) = fixture().await;

	reopened.store = SqliteStore::open(&root.paths()).unwrap();

	assert!(matches!(
		reopened
			.continue_misalignment("agent", review.clone(), "confirm", "old-source-review")
			.await,
		Err(AgentError::Rejected(_))
	));
	assert!(requests.try_recv().is_err());
	assert_eq!(reopened.store.agent_misalignment("agent".into()).await.unwrap(), Some(review));
	assert_eq!(
		reopened.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Idle
	);
}

#[tokio::test]
async fn idle_thread_recovery_restores_only_latest_misalignment_failure() {
	for (known, stopped, has_old) in [
		(false, false, true),
		(false, true, true),
		(true, false, true),
		(true, true, true),
		(true, false, false),
	] {
		let error = serde_json::json!({"codexErrorInfo":"misalignmentPolicyViolation","misalignment":{"detailedExplanation":"Recovered findings","steer":{"message":"Clarified scope"}}});
		let mut history = serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"opaque turn/1","status":"failed","error":error,"items":[]},{"id":"latest","status":if stopped {"failed"} else {"completed"},"error":if stopped {error.clone()} else {Value::Null},"items":[]}]}}});

		if !has_old {
			history["opaque thread/1"]["thread"]["turns"].as_array_mut().unwrap().remove(0);
		}

		let (mut agent, mut sent, directory) = fixture_with_history(history).await;

		agent.start_agent("agent", "Coordinate").await.unwrap();

		if known {
			agent.observe_misalignment("opaque thread/1", "opaque turn/1", &error).await.unwrap();
		}

		agent.store.complete_agent_turn("agent".into(), "opaque turn/1".into()).await.unwrap();

		while sent.try_recv().is_ok() {}

		agent.recover_persisted().await.unwrap();

		let precaution = agent.store.agent_misalignment("agent".into()).await.unwrap();

		assert_eq!(precaution.is_some(), stopped || (known && !has_old));

		let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
		let reopened = SqliteStore::open(&root.paths()).unwrap();

		assert_eq!(reopened.agent_misalignment("agent".into()).await.unwrap(), precaution);

		if let Some(precaution) = precaution {
			assert_eq!(precaution.turn_id, if stopped { "latest" } else { "opaque turn/1" });
			assert!(precaution.details_json.unwrap().contains("Recovered findings"));
		}

		while let Ok(request) = sent.try_recv() {
			assert_eq!(request["method"], "thread/read");
		}
	}
}

#[tokio::test]
async fn misalignment_does_not_send_or_consume_pending_provider_approval() {
	let (mut agent, mut sent, _directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	let id = RequestId::Number(7);

	agent.handle_event(ServerEvent::Request {id:id.clone(),method:"item/commandExecution/requestApproval".into(),params:serde_json::json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","itemId":"item","command":"pwd"})}).await.unwrap();

	let event = agent.pending_requests[&id];

	agent
		.observe_misalignment(
			"opaque thread/1",
			"opaque turn/1",
			&serde_json::json!({"codexErrorInfo":"misalignmentPolicyViolation"}),
		)
		.await
		.unwrap();

	while sent.try_recv().is_ok() {}

	assert!(
		agent.respond_pending_event(event, serde_json::json!({"decision":"accept"})).await.is_err()
	);
	assert_eq!(agent.pending_requests[&id], event);
	assert!(agent.store.get_agent_inbox_event(event).await.unwrap().disposition.is_none());
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn mcp_form_response_validates_original_schema_before_consuming_live_request() {
	for mode in ["form", "openai/form", "openaiForm"] {
		let (mut agent, _old_sent, _directory) = fixture().await;

		agent.start_agent("agent", "Coordinate").await.unwrap();
		agent.store.complete_agent_turn("agent".into(), "opaque turn/1".into()).await.unwrap();

		let id = RequestId::String("mcp-form".into());
		let mut sent = attach_request_transport(&mut agent, serde_json::json!({}), serde_json::json!({"id":id,"method":"mcpServer/elicitation/request","params":{"threadId":"opaque thread/1","turnId":null,"serverName":"test","mode":mode,"requestedSchema":{"type":"object","properties":{"allow":{"type":"boolean"}},"required":["allow"]}}})).await;
		let event = agent.pending_requests[&id];

		while sent.try_recv().is_ok() {}

		for response in [
			serde_json::json!({"action":"accept","content":{"allow":"true"}}),
			serde_json::json!({"action":"accept","content":{"allow":true},"_meta":{"persist":"always"}}),
			serde_json::json!({"decision":"accept"}),
		] {
			assert!(matches!(
				agent.respond_pending_event(event, response).await,
				Err(AgentError::Rejected(_))
			));
			assert_eq!(agent.pending_requests[&id], event);
			assert!(sent.try_recv().is_err());
		}

		agent
			.respond_pending_event(
				event,
				serde_json::json!({"action":"accept","content":{"allow":false},"_meta":null}),
			)
			.await
			.unwrap();

		let reply = sent.recv().await.unwrap();

		assert_eq!(reply["id"], "mcp-form");
		assert_eq!(reply["result"]["content"]["allow"], false);
		assert!(!agent.pending_requests.contains_key(&id));
		assert!(
			agent
				.respond_pending_event(event, serde_json::json!({"action":"cancel","content":null}))
				.await
				.is_err()
		);
		assert!(sent.try_recv().is_err());
	}
}

#[tokio::test]
async fn standalone_mcp_resolution_and_reconnection_never_replay_a_reply() {
	for mode in ["form", "openai/form", "openaiForm"] {
		let (mut agent, mut sent, _directory) = fixture().await;

		agent.start_agent("agent", "Coordinate").await.unwrap();
		agent.store.complete_agent_turn("agent".into(), "opaque turn/1".into()).await.unwrap();

		let id = RequestId::Number(17);
		let params = serde_json::json!({"threadId":"opaque thread/1","turnId":null,"serverName":"test","mode":mode,"requestedSchema":null});

		agent
			.handle_event(ServerEvent::Request {
				id: id.clone(),
				method: "mcpServer/elicitation/request".into(),
				params: params.clone(),
			})
			.await
			.unwrap();

		let old_event = agent.pending_requests[&id];
		let mut reconnected =
			AgentCoordinator::new(agent.store.clone(), agent.client.clone(), agent.config.clone())
				.unwrap();

		while sent.try_recv().is_ok() {}

		assert!(
			reconnected
				.respond_pending_event(
					old_event,
					serde_json::json!({"action":"accept","content":null})
				)
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
				params: serde_json::json!({"threadId":"wrong-thread","requestId":17}),
			})
			.await
			.unwrap();

		assert_eq!(reconnected.pending_requests[&id], event);

		reconnected
			.handle_event(ServerEvent::Notification {
				method: "serverRequest/resolved".into(),
				params: serde_json::json!({"threadId":"opaque thread/1","requestId":17}),
			})
			.await
			.unwrap();

		assert!(!reconnected.pending_requests.contains_key(&id));
		assert_eq!(
			reconnected.store.get_agent_inbox_event(event).await.unwrap().disposition,
			Some(AgentDisposition::Resolved)
		);
		assert!(
			reconnected
				.respond_pending_event(event, serde_json::json!({"action":"accept","content":null}))
				.await
				.is_err()
		);
		assert!(sent.try_recv().is_err());
	}
}

#[tokio::test]
async fn async_question_skip_is_source_bound_durable_and_never_a_native_answer() {
	let (mut agent, mut sent, directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let item = serde_json::json!({"type":"agentMessage","delivery":"async","id":"skip-source","questions":[{"title":"First?"},{"title":"Second?"}]});

	agent.observe_async_question_item("opaque thread/1", "opaque turn/1", &item).await.unwrap();

	let first = decodex_protocol::agent_async_question_id("skip-source", 0);
	let second = decodex_protocol::agent_async_question_id("skip-source", 1);

	assert!(agent.skip_async_question("agent", "other-thread", &first).await.is_err());
	assert!(agent.skip_async_question("other-work", "opaque thread/1", &first).await.is_err());

	agent.dispatch_paused = true;

	assert!(agent.skip_async_question("agent", "opaque thread/1", &first).await.is_err());

	agent.dispatch_paused = false;

	agent.skip_async_question("agent", "opaque thread/1", &first).await.unwrap();
	agent.skip_async_question("agent", "opaque thread/1", &first).await.unwrap();
	agent.observe_async_question_item("opaque thread/1", "opaque turn/1", &item).await.unwrap();

	let pending = agent.store.read_agent_async_questions("agent".into()).await.unwrap();

	assert_eq!(pending.len(), 1);
	assert_eq!(pending[0].question_id, second);
	assert!(agent.answer_async_question("agent", &first, "Must not send", "key").await.is_err());

	let paths =
		DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap().paths();
	let reopened = SqliteStore::open(&paths).unwrap();

	assert_eq!(
		reopened.read_agent_async_questions("agent".into()).await.unwrap()[0].question_id,
		second
	);

	let db = Connection::open(paths.product_database_file()).unwrap();

	assert_eq!(
		db.query_row("SELECT count(*) FROM agent_async_answers", [], |row| row.get::<_, i64>(0))
			.unwrap(),
		0
	);
	assert_eq!(
		db.query_row("SELECT count(*) FROM agent_async_skips", [], |row| row.get::<_, i64>(0))
			.unwrap(),
		1
	);
	assert!(agent.store.list_pending_agent_events(100).await.unwrap().is_empty());
	assert!(sent.try_recv().is_err());

	// Recovery and transport-uncertain answers cannot be hidden by a skip.
	reopened
		.request_agent_async_recovery("opaque thread/1".into(), "skip-source".into())
		.await
		.unwrap();

	assert!(agent.skip_async_question("agent", "opaque thread/1", &second).await.is_err());

	reopened.finish_agent_async_recovery("agent".into(), "opaque thread/1".into()).await.unwrap();
	reopened
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "pending-answer".into(),
			work_item_id: "agent".into(),
			event_kind: "steer_pending".into(),
			payload: serde_json::json!({"asyncQuestionId":second}).to_string(),
		})
		.await
		.unwrap();

	assert!(agent.skip_async_question("agent", "opaque thread/1", &second).await.is_err());
	assert_eq!(reopened.read_agent_async_questions("agent".into()).await.unwrap().len(), 1);
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn skipped_question_survives_rebuild_only_while_native_content_is_unchanged() {
	let (mut agent, mut sent, directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let mut item = serde_json::json!({"type":"agentMessage","delivery":"async","id":"skip-rebuild","questions":[{"title":"Original?"}]});

	agent.observe_async_question_item("opaque thread/1", "opaque turn/1", &item).await.unwrap();

	let id = decodex_protocol::agent_async_question_id("skip-rebuild", 0);

	for (step, expected) in [(0, 0), (1, 1), (2, 0)] {
		agent.skip_async_question("agent", "opaque thread/1", &id).await.unwrap();

		let mut projection = Projection::default();

		if step == 1 {
			item["questions"][0]["title"] = serde_json::json!("Changed?");
		}
		if step < 2 {
			projection.observe("opaque thread/1", "opaque turn/1", &item).unwrap();
		}

		agent.store.refresh_agent_async_projection("opaque thread/1".into()).await.unwrap();

		assert!(
			agent
				.store
				.replace_agent_async_projection(
					"agent".into(),
					"opaque thread/1".into(),
					None,
					projection.questions,
					vec![]
				)
				.await
				.unwrap()
		);

		let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
		let reopened = SqliteStore::open(&root.paths()).unwrap();

		assert_eq!(
			reopened.read_agent_async_questions("agent".into()).await.unwrap().len(),
			expected
		);
	}

	assert!(sent.try_recv().is_err(), "local dismissal never sends model input");
}

#[tokio::test]
async fn unfinished_native_text_keeps_source_and_display_only_status_after_reopen() {
	for final_readback in ["missing", "complete", "empty", "complete_plan"] {
		let items = if final_readback == "complete_plan" {
			serde_json::json!([{"id":"answer","type":"agentMessage","text":"Authoritative final answer"},
				{"id":"plan","type":"plan","text":"Authoritative final plan"}])
		} else if final_readback != "missing" {
			serde_json::json!([{"id":"answer","type":"agentMessage","text":if final_readback == "complete" { "Authoritative final answer" } else { "" }}])
		} else {
			serde_json::json!([])
		};
		let history = serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[{"id":"opaque turn/1","status":"interrupted","items":items}]}}});
		let (mut agent, mut sent, directory) = fixture_with_history(history).await;

		agent.start_agent("agent", "Talk").await.unwrap();

		while sent.try_recv().is_ok() {}

		let source = "Unfinished $$\\frac{a}{b}";

		for (id, method) in [("answer", "item/agentMessage/delta"), ("plan", "item/plan/delta")] {
			agent.handle_event(ServerEvent::Notification {
			method: method.into(),
			params: serde_json::json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","itemId":id,"delta":source}),
		}).await.unwrap();
		}

		let live = agent.store.read_agent_output("agent".into()).await.unwrap();

		assert_eq!(
			live.iter().map(|item| item.kind.as_str()).collect::<Vec<_>>(),
			["agentMessage", "plan"]
		);

		agent.handle_event(ServerEvent::Notification {
		method: "turn/completed".into(),
		params: serde_json::json!({"threadId":"opaque thread/1","turn":{"id":"opaque turn/1","status":"interrupted","items":[]}}),
	}).await.unwrap();

		let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
		let reopened = SqliteStore::open(&root.paths()).unwrap();
		let events = reopened.read_agent_transcript("agent".into(), None, 32).await.unwrap().0;
		let rendered = application::render_agent_history_for_test(events);
		let partial: Vec<_> =
			rendered.iter().filter(|entry| entry.kind.starts_with("partial_")).collect();

		assert_eq!(
			partial.len(),
			match final_readback {
				"complete_plan" => 0,
				"complete" => 1,
				_ => 2,
			}
		);

		for entry in partial {
			assert_eq!(entry.text, source);
			assert_eq!(entry.turn_id.as_deref(), Some("opaque turn/1"));

			let identity = entry.native_source.as_ref().unwrap();

			assert_eq!(identity.thread_id, "opaque thread/1");
			assert_eq!(identity.turn_id, "opaque turn/1");
			assert_eq!(
				identity.item_id,
				if entry.kind == "partial_plan" { "plan" } else { "answer" }
			);
			assert!(entry.receipt.as_ref().unwrap().disposed);
		}

		assert!(
			reopened
				.list_pending_agent_events(32)
				.await
				.unwrap()
				.iter()
				.all(|event| event.event_kind != "partial_output")
		);

		while let Ok(request) = sent.try_recv() {
			assert_ne!(request["method"], "turn/start");
		}
	}
}

#[tokio::test]
async fn missed_native_active_turn_recovery_rejects_reverted_readback() {
	for reverted in [false, true] {
		let history = serde_json::json!({"_misalignment_revert_on_read":reverted,"opaque thread/1":{"thread":{"id":"opaque thread/1","status":{"type":"active"},"turns":[{"id":"native-turn","status":"inProgress","items":[]}]}}});
		let (mut agent, mut sent, _home) = fixture_with_history(history).await;

		agent.start_agent("agent", "Original user input").await.unwrap();

		complete(&mut agent, "agent").await;

		while sent.try_recv().is_ok() {}

		agent.recover_native_turns().await.unwrap();

		let work = agent.store.get_agent_work_item("agent".into()).await.unwrap();

		assert_eq!(
			work.active_turn_id.as_deref(),
			if reverted { None } else { Some("native-turn") }
		);

		while let Ok(request) = sent.try_recv() {
			assert!(
				!["thread/start", "turn/start", "turn/steer", "thread/inject_items"]
					.contains(&request["method"].as_str().unwrap())
			);

			if request["method"] == "thread/resume" {
				for field in ["cwd", "model", "sandbox", "approvalPolicy", "config"] {
					assert!(request["params"].get(field).is_none());
				}
			}
		}
	}
}

#[tokio::test]
async fn live_plan_finality_and_kind_survive_restart() {
	let (mut coordinator, _sent, directory) = fixture().await;
	let work = coordinator.start_agent("agent", "Plan").await.unwrap();
	let turn = work.active_turn_id.as_deref().unwrap();
	let delta = |turn: &str, text: &str| ServerEvent::Notification {
		method: "item/plan/delta".into(),
		params: serde_json::json!({"threadId":work.codex_thread_id,"turnId":turn,"itemId":"plan","delta":text}),
	};

	coordinator.handle_event(delta("wrong-turn", "Wrong")).await.unwrap();

	assert!(coordinator.store.read_agent_output("agent".into()).await.unwrap().is_empty());

	coordinator.handle_event(delta(turn, &"界".repeat(30_000))).await.unwrap();

	let partial = coordinator.store.read_agent_output("agent".into()).await.unwrap();

	assert_eq!(partial[0].kind, "plan");
	assert!(partial[0].truncated && partial[0].text.len() <= 65_536);

	coordinator.handle_event(ServerEvent::Notification {
        method: "item/completed".into(),
        params: serde_json::json!({"threadId":work.codex_thread_id,"turnId":turn,"item":{"id":"plan","type":"plan","text":"Final plan"}}),
    }).await.unwrap();

	let thread = work.codex_thread_id.unwrap();
	let turn = turn.to_owned();

	drop(coordinator);

	let paths =
		DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap().paths();
	let store = SqliteStore::open(&paths).unwrap();

	store
		.update_agent_output_record(AgentOutputUpdate {
			thread_id: thread.clone(),
			turn_id: turn.clone(),
			item_id: "plan".into(),
			kind: "plan".into(),
			text: "Late draft".into(),
			completed: false,
		})
		.await
		.unwrap();

	assert!(
		store
			.update_agent_output(thread, turn, "plan".into(), "Wrong kind".into(), true)
			.await
			.is_err()
	);

	let saved = store.read_agent_output("agent".into()).await.unwrap();

	assert_eq!(saved.len(), 1);
	assert_eq!(saved[0].text, "Final plan");
	assert_eq!(saved[0].kind, "plan");
	assert!(!saved[0].truncated);
}
