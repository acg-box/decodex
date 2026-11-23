//! Registered and locally created Agents use real native turns and the same Work authority.
use std::{
	collections::VecDeque,
	env,
	path::Path,
	sync::{Arc, Mutex},
	time::Duration,
};

use super::{NativeSession, ServerEvent, Value, project_work};
use crate::{
	agent::{AgentConfig, AgentCoordinator},
	agent_model_settings::tests::OwnedReviewer,
};
use decodex_codex::app_server_client::InitializeCapabilities;
use decodex_core::{
	DecodexRoot,
	work::{ResultSource, ThreadRegistration, WorkAssessment, WorkCommand, WorkResult, WorkState},
};
use decodex_database::{AgentDisposition, EnqueueAgentEvent, SqliteStore};
use rusqlite::Connection;
use serde_json::json;
use tempfile::TempDir;
use tokio::task::JoinHandle;

const MCP: Option<&str> = Some("mcp__decodex");
type ModelOutput = Arc<Mutex<VecDeque<Value>>>;
type ModelRequests = Arc<Mutex<Vec<Value>>>;
const DOMAIN_RULE: &str = "Use the external archive as the source of historical facts.";

fn coordinator(store: &SqliteStore, session: &NativeSession, home: &Path) -> AgentCoordinator {
	let mut config =
		AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), home.display().to_string());
	config.sandbox = "read-only".into();
	AgentCoordinator::new(store.clone(), session.client.clone(), config).unwrap()
}

fn create_call(id: &str, owner: &str, decision: i64, namespace: Option<&str>) -> Value {
	let command = json!({"operation":"create","work":{
		"id":id,"owner_agent_id":owner,"title":id,"state":"open","user_hold":false,
		"requirements":{"id":format!("{id}-r1"),"goal":"Review the fixture",
			"criteria":"Return one finding","source":format!("event:{decision}")}
	}});
	work_call(id, command, namespace)
}

fn work_call(id: &str, command: Value, namespace: Option<&str>) -> Value {
	let args = json!({"commandId":id,"command":command});
	let mut call = json!({"type":"function_call","name":"work_manage","call_id":id,"arguments":args.to_string()});
	if let Some(namespace) = namespace {
		call["namespace"] = json!(namespace);
	}
	call
}

fn read_work(actor: &str) -> Value {
	json!({"type":"function_call","namespace":MCP,"name":"work_read","call_id":format!("read-{actor}"),"arguments":"{}"})
}

async fn native_turn(session: &NativeSession, thread: &str, turn: &str) -> Value {
	let history = session.client.thread_read_turn(thread, turn).await.unwrap();
	history["thread"]["turns"]
		.as_array()
		.unwrap()
		.iter()
		.find(|row| row["id"] == turn)
		.unwrap()
		.clone()
}

fn reopen(home: &Path) -> NativeSession {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").unwrap();
	NativeSession::with_capabilities(&binary, home, InitializeCapabilities::for_agent())
}

async fn setup() -> (TempDir, NativeSession, ModelOutput, ModelRequests, JoinHandle<()>) {
	let next = ModelOutput::default();
	let output = next.clone();
	let requests = ModelRequests::default();
	let (home, original, _, _, backend) =
		project_work::setup_with_requests(Some(requests.clone()), move |serial| {
			output.lock().unwrap().pop_front().unwrap_or_else(|| {
				json!({"type":"message","role":"assistant",
			"id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Fixture complete"}]})
			})
		})
		.await;
	(home, original, next, requests, backend)
}

async fn external_conversation(session: &mut NativeSession, home: &Path) -> (String, String) {
	let old = session
		.client
		.thread_start(json!({
			"cwd":home,"model":"gpt-5.6-sol","approvalPolicy":"never","sandbox":"read-only",
			"developerInstructions":DOMAIN_RULE,"dynamicTools":[{"name":"work_manage","description":"Tool provided by the original client",
				"inputSchema":{"type":"object","properties":{}}}]
		}))
		.await
		.unwrap();
	let imported = old["thread"]["id"].as_str().unwrap();
	let response = session
		.client
		.turn_start(json!({"threadId":imported,
			"input":[{"type":"text","text":"Historical user input"}]}))
		.await
		.unwrap();
	let historical = response["turn"]["id"].as_str().unwrap();
	while let Some(event) = session.events.recv().await {
		if matches!(&event, ServerEvent::Notification { method, params }
				if method == "turn/completed" && params["turn"]["id"] == historical)
		{
			break;
		}
	}
	(imported.to_owned(), historical.to_owned())
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; native thread registration and real Work authority"]
async fn registered_and_created_agents_create_work_after_cold_resume() {
	let (home, mut original, next, requests, backend) = setup().await;
	tokio::time::timeout(Duration::from_secs(90), async {
		let (thread_id, turn_id) = external_conversation(&mut original, home.path()).await;
		let (imported, historical) = (thread_id.as_str(), turn_id.as_str());
		let retained = native_turn(&original, imported, historical).await;
		let mut session = reopen(home.path());
		let root = DecodexRoot::new(home.path().canonicalize().unwrap().join("state")).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		let mut agent = coordinator(&store, &session, home.path());
		let adopted = agent
			.register_native_thread(imported, ThreadRegistration::Promote { expected_owner: None })
			.await
			.unwrap();
		assert_eq!(adopted.codex_thread_id.as_deref(), Some(imported));
		assert!(adopted.active_turn_id.is_none());
		assert!(store.read_agent_work_events(adopted.id.clone(), 10).await.unwrap().is_empty());
		agent
			.enqueue_user_message(&adopted.id, "busy", "Hold this input until I resend it")
			.await
			.unwrap();
		agent.wake_pending().await.unwrap();
		let held = store
			.read_agent_work_events(adopted.id.clone(), 10)
			.await
			.unwrap()
			.into_iter()
			.find(|event| event.event_kind == "user_message")
			.unwrap();
		assert_eq!(held.disposition, Some(AgentDisposition::UserDecision));
		assert!(held.delivered_turn_id.is_none());
		assert_eq!(store.list_agent_thread_in_use_events().await.unwrap().len(), 1);
		assert_eq!(native_turn(&original, imported, historical).await, retained);
		drop(original);

		AgentCoordinator::create_conversation(
			&store,
			"created",
			EnqueueAgentEvent {
				work_item_id: "created".into(),
				source_event_id: "first-input".into(),
				event_kind: "user_message".into(),
				payload: json!({"source":"user","text":"Start this conversation"}).to_string(),
			},
			None,
		)
		.await
		.unwrap();
		store.set_assistant("created".into(), true).await.unwrap();
		agent.wake_pending().await.unwrap();
		let created =
			store.get_agent_work_item("created".into()).await.unwrap().codex_thread_id.unwrap();
		project_work::finish(&mut session, &mut agent, &created).await;
		drop(agent);
		drop(session);
		drop(store);

		let mut session = reopen(home.path());
		let authority =
			OwnedReviewer::new(home.path(), &session.client, "fixture-host", "host-turn").await;
		let reopened = &authority.store;
		let mut agent = coordinator(reopened, &session, home.path());
		agent.bind_native_generation(authority.key.generation.clone());
		for (actor, thread, work) in [
			("created", created.as_str(), "created-work"),
			(adopted.id.as_str(), imported, "registered-work"),
		] {
			let decision = user_decision(&mut agent, reopened, actor, work).await;
			let foreign = format!("{work}-foreign");
			let mut calls = vec![
				create_call(work, actor, decision, MCP),
				create_call(&foreign, actor, decision, None),
			];
			if thread == imported {
				calls.push(create_call("wrong-owner", "created", decision, MCP));
			}
			next.lock().unwrap().extend([read_work(actor), Value::Array(calls)]);
			let history = deliver(&mut agent, &mut session, reopened, actor, thread).await;
			assert_model_contract(&requests, &history, Some(actor), thread == imported);
			let catalog = session
				.client
				.request(
					"mcpServerStatus/list",
					json!({"threadId":thread,"serverName":"decodex","detail":"toolsAndAuthOnly"}),
				)
				.await
				.unwrap();
			let tools = catalog["data"][0]["tools"].as_object().expect("thread MCP tool catalog");
			assert!(tools.contains_key("work_manage"));
			for private in ["assistant_read", "assistant_manage", "assistant_memory"] {
				assert_eq!(
					tools.contains_key(private),
					actor == "created",
					"Personal Assistant tool {private}"
				);
			}
			let items = history["items"].as_array().unwrap();
			assert_mcp_call(items, work);
			if thread == imported {
				let old = items
					.iter()
					.find(|item| item["id"] == foreign)
					.expect("original client tool call");
				assert_eq!(old["type"], "dynamicToolCall");
				assert_eq!(old["success"], false);
				assert_mcp_call(items, "wrong-owner");
			}
			assert!(
				reopened
					.agent_thread_is_owned(
						actor.into(),
						thread.into(),
						Some(authority.key.generation.as_str().into())
					)
					.await
					.unwrap()
			);
			let result = reopened.work_portfolio(Some(work.into())).await.unwrap();
			assert_eq!(result.works.len(), 1, "Expected a Work from {actor} on {thread}");
			assert_eq!(result.works[0].owner_agent_id, actor);
			assert_eq!(result.works[0].requirements.source, format!("event:{decision}"));
		}
		let db = Connection::open(root.paths().product_database_file()).unwrap();
		let mut stmt = db.prepare("SELECT id,agent_id FROM work_commands ORDER BY id").unwrap();
		let commands = stmt
			.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
			.unwrap()
			.collect::<Result<Vec<_>, _>>()
			.unwrap();
		assert_eq!(
			commands,
			vec![
				("created-work".into(), "created".into()),
				("registered-work".into(), adopted.id.clone())
			]
		);
		assert_eq!(reopened.work_portfolio(None).await.unwrap().works.len(), 2);
		assert!(reopened.get_agent_inbox_event(held.id).await.unwrap().delivered_turn_id.is_none());
		assert_eq!(native_turn(&session, imported, historical).await, retained);
		let (result, assessment) = complete_registered_work(
			&mut agent,
			&mut session,
			reopened,
			&next,
			&adopted.id,
			imported,
		)
		.await;
		drop(stmt);
		drop(db);
		drop(agent);
		drop(session);
		drop(authority);
		let restored = SqliteStore::open(&root.paths()).unwrap();
		let work = restored.work_portfolio(Some(result.work_id.clone())).await.unwrap();
		assert_eq!(work.works.len(), 1);
		assert_eq!(work.works[0].state, WorkState::Completed);
		assert_eq!(work.works[0].owner_agent_id, adopted.id);
		assert_eq!(work.works[0].assessment.as_ref(), Some(&assessment));
		assert_eq!(work.results, [result]);
	})
	.await
	.expect("bounded native registration and Work acceptance");
	backend.abort();
}

async fn complete_registered_work(
	agent: &mut AgentCoordinator,
	session: &mut NativeSession,
	store: &SqliteStore,
	next: &ModelOutput,
	actor: &str,
	thread: &str,
) -> (WorkResult, WorkAssessment) {
	let finding = "The historical user turn is still readable from the registered conversation.";
	agent
		.enqueue_user_message(
			actor,
			"inspect-history",
			"Review the retained conversation and report one finding.",
		)
		.await
		.unwrap();
	next.lock()
		.unwrap()
		.push_back(json!({"type":"message","role":"assistant","id":"history-finding",
		"content":[{"type":"output_text","text":finding}]}));
	let history = deliver(agent, session, store, actor, thread).await;
	let item = history["items"]
		.as_array()
		.unwrap()
		.iter()
		.find(|i| i["type"] == "agentMessage" && i["text"] == finding)
		.unwrap();
	let result = WorkResult {
		id: "history-result".into(),
		work_id: "registered-work".into(),
		requirements_id: "registered-work-r1".into(),
		instruction_id: None,
		native_contribution_id: None,
		agent_id: actor.into(),
		summary: finding.into(),
		source: ResultSource::NativeItem {
			thread_id: thread.into(),
			turn_id: history["id"].as_str().unwrap().into(),
			item_id: item["id"].as_str().unwrap().into(),
		},
		limits: "Local fixture history only".into(),
	};
	let assessment = WorkAssessment {
		requirements_id: result.requirements_id.clone(),
		result_ids: vec![result.id.clone()],
		summary: "The retained turn supplies the requested finding.".into(),
		limits: result.limits.clone(),
	};
	for (id, message, command) in [
		(
			"record-finding",
			"Record the finding with its original evidence.",
			WorkCommand::RecordResult { result: result.clone() },
		),
		(
			"complete-review",
			"Assess the recorded result and complete the review.",
			WorkCommand::Complete {
				work_id: result.work_id.clone(),
				expected_requirements: result.requirements_id.clone(),
				assessment: assessment.clone(),
			},
		),
	] {
		agent.enqueue_user_message(actor, id, message).await.unwrap();
		next.lock().unwrap().push_back(work_call(id, serde_json::to_value(command).unwrap(), MCP));
		let turn = deliver(agent, session, store, actor, thread).await;
		assert_mcp_call(turn["items"].as_array().unwrap(), id);
	}
	(result, assessment)
}

async fn user_decision(
	agent: &mut AgentCoordinator,
	store: &SqliteStore,
	actor: &str,
	work: &str,
) -> i64 {
	agent.enqueue_user_message(actor, work, "Create a Work to review the fixture").await.unwrap();
	store
		.list_undelivered_agent_events(10)
		.await
		.unwrap()
		.into_iter()
		.find(|e| e.work_item_id == actor && e.event_kind == "user_message")
		.unwrap()
		.id
}

async fn deliver(
	agent: &mut AgentCoordinator,
	session: &mut NativeSession,
	store: &SqliteStore,
	actor: &str,
	thread: &str,
) -> Value {
	agent.wake_pending().await.unwrap();
	let turn = store.get_agent_work_item(actor.into()).await.unwrap().active_turn_id.unwrap();
	project_work::finish(session, agent, thread).await;
	native_turn(session, thread, &turn).await
}

fn assert_mcp_call(items: &[Value], id: &str) {
	let call = items
		.iter()
		.find(|item| item["id"] == id)
		.unwrap_or_else(|| panic!("Missing tool call {id}: {items:?}"));
	assert_eq!(call["type"], "mcpToolCall");
	assert_eq!(call["tool"], "work_manage");
	assert_eq!(call["server"], "decodex");
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; native in-process promotion"]
async fn promoting_a_loaded_ordinary_thread_enables_work_without_a_new_process() {
	let (home, mut original, next, requests, backend) = setup().await;
	tokio::time::timeout(Duration::from_secs(90), async {
		let (thread, _) = external_conversation(&mut original, home.path()).await;
		drop(original);
		let mut session = reopen(home.path());
		let authority =
			OwnedReviewer::new(home.path(), &session.client, "fixture-host", "host-turn").await;
		let store = &authority.store;
		let mut agent = coordinator(store, &session, home.path());
		agent.bind_native_generation(authority.key.generation.clone());
		let ordinary =
			agent.register_native_thread(&thread, ThreadRegistration::Register).await.unwrap();
		let actor = &ordinary.id;
		let decision = user_decision(&mut agent, store, actor, "ordinary").await;
		next.lock().unwrap().push_back(create_call("before-promotion", actor, decision, MCP));
		let ordinary_turn = deliver(&mut agent, &mut session, store, actor, &thread).await;
		assert_model_contract(&requests, &ordinary_turn, None, true);
		assert!(store.work_portfolio(None).await.unwrap().works.is_empty());
		let promoted = agent
			.register_native_thread(&thread, ThreadRegistration::Promote { expected_owner: None })
			.await
			.unwrap();
		assert_eq!(&promoted.id, actor);
		let decision = user_decision(&mut agent, store, actor, "hot-work").await;
		next.lock()
			.unwrap()
			.extend([read_work(actor), create_call("hot-work", actor, decision, MCP)]);
		let history = deliver(&mut agent, &mut session, store, actor, &thread).await;
		assert_model_contract(&requests, &history, Some(actor), true);
		assert_mcp_call(history["items"].as_array().unwrap(), "hot-work");
		let result = store.work_portfolio(None).await.unwrap();
		assert_eq!(result.works.len(), 1);
		assert_eq!(result.works[0].id, "hot-work");
		assert_eq!(&result.works[0].owner_agent_id, actor);
		assert_eq!(result.works[0].requirements.source, format!("event:{decision}"));
	})
	.await
	.expect("bounded native promotion");
	backend.abort();
}

fn assert_model_contract(
	requests: &ModelRequests,
	turn: &Value,
	actor: Option<&str>,
	external: bool,
) {
	let requests = requests.lock().unwrap();
	let mut bodies = requests.iter().filter(|body| {
		body["client_metadata"]["x-codex-turn-metadata"]
			.as_str()
			.and_then(|metadata| serde_json::from_str::<Value>(metadata).ok())
			.is_some_and(|metadata| metadata["turn_id"] == turn["id"])
	});
	let first = bodies.next().expect("first native Responses request for this turn");
	let input = first["input"].to_string();
	assert_eq!(
		input.contains("You are the accountable Decodex Agent for this turn"),
		actor.is_some(),
		"Decodex role visible in the first model input"
	);
	let domain = if external {
		DOMAIN_RULE
	} else {
		"you are a persistent Agent with one current native conversation"
	};
	assert!(
		input.contains(domain)
			|| first["instructions"].as_str().is_some_and(|s| s.contains(domain)),
		"retained native developer instruction"
	);
	if let Some(actor) = actor {
		let call_id = format!("read-{actor}");
		let output = bodies
			.flat_map(|body| body["input"].as_array().unwrap())
			.find(|item| item["type"] == "function_call_output" && item["call_id"] == call_id)
			.expect("real work_read reply delivered to the model");
		let contract = output["output"].to_string();
		for field in [
			"commandContract",
			"Example create arguments",
			"operation",
			"owner_agent_id",
			"criteria",
			"record_result",
			"complete",
			"agentId",
			actor,
		] {
			assert!(contract.contains(field), "Missing Work contract field {field} in {contract}");
		}
	}
}

#[tokio::test]
#[ignore = "requires the pinned Codex CLI and its Code Mode helper"]
async fn promoted_exec_discovers_and_uses_work_in_code_mode() {
	let (home, original, next, requests, backend) = setup().await;
	drop(original);
	let config = home.path().join("config.toml");
	let contents = std::fs::read_to_string(&config)
		.unwrap()
		.replace("[features]", "[features]\ncode_mode=true");
	std::fs::write(config, contents).unwrap();
	tokio::time::timeout(Duration::from_secs(60), async {
		let output =
			tokio::process::Command::new(env::var_os("DECODEX_TEST_CODEX_BINARY").unwrap())
				.args([
					"exec",
					"--json",
					"--skip-git-repo-check",
					"--sandbox",
					"read-only",
					"--model",
					"gpt-6.2-sol",
					"Record historical fixture input",
				])
				.env_clear()
				.env("HOME", home.path())
				.env("CODEX_HOME", home.path())
				.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
				.current_dir(home.path())
				.kill_on_drop(true)
				.output()
				.await
				.unwrap();
		assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
		let thread = String::from_utf8(output.stdout)
			.unwrap()
			.lines()
			.filter_map(|line| serde_json::from_str::<Value>(line).ok())
			.find_map(|event| event["thread_id"].as_str().map(str::to_owned))
			.expect("exec thread");
		let mut session = reopen(home.path());
		let authority =
			OwnedReviewer::new(home.path(), &session.client, "fixture-host", "host-turn").await;
		let mut agent = coordinator(&authority.store, &session, home.path());
		agent.bind_native_generation(authority.key.generation.clone());
		let adopted = agent
			.register_native_thread(&thread, ThreadRegistration::Promote { expected_owner: None })
			.await
			.unwrap();

		// Opening chat history can trigger a prompt suggestion before the first user input.
		let history = session.client.thread_history_page(&thread, None, 2).await.unwrap();
		assert_eq!(history["thread"]["id"], thread);
		agent.prepare_sample_source(&thread).await.unwrap();
		let decision = user_decision(&mut agent, &authority.store, &adopted.id, "exec-work").await;
		let args = create_call("exec-work", &adopted.id, decision, MCP);
		let args = args["arguments"].as_str().unwrap();
		let input = format!(
			concat!(
				"text(ALL_TOOLS.filter(t => t.name.includes('mcp__decodex__')).map(t => t.name));",
				"text(await tools.mcp__decodex__work_read({{}}));",
				"text(await tools.mcp__decodex__work_manage({}));"
			),
			args
		);
		next.lock().unwrap().push_back(json!({
			"type": "custom_tool_call", "name": "exec", "call_id": "exec-tools", "input": input,
		}));
		let history =
			deliver(&mut agent, &mut session, &authority.store, &adopted.id, &thread).await;
		let output = requests
			.lock()
			.unwrap()
			.iter()
			.flat_map(|body| body["input"].as_array().unwrap())
			.find(|item| {
				item["type"] == "custom_tool_call_output" && item["call_id"] == "exec-tools"
			})
			.expect("Code Mode output")
			.to_string();
		for field in ["mcp__decodex__work_read", "mcp__decodex__work_manage", "commandContract"] {
			assert!(output.contains(field), "Code Mode did not return {field}: {output}");
		}
		let work = authority.store.work_portfolio(Some("exec-work".into())).await.unwrap();
		assert_eq!(work.works.len(), 1, "{history}");
	})
	.await
	.expect("managed Code Mode acceptance");
	backend.abort();
}
