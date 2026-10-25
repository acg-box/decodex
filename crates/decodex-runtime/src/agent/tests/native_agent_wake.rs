//! A native child result resumes an idle root through the ordinary event bridge.
use std::{env, fs, panic::AssertUnwindSafe, time::Duration};

use futures_util::FutureExt as _;
use tokio::{net::TcpListener, process::Command, sync::mpsc, time};

use crate::agent::{
	tests,
	tests::{
		AgentConfig, AgentCoordinator, AppServerClient, AsyncWriteExt, ServerEvent, Value,
		native_task_references,
	},
};
use decodex_core::work::{
	NativeContribution, ResultSource, Work, WorkCommand, WorkRequirements, WorkResult, WorkState,
};
use decodex_database::AgentDispatchState;
const NATIVE_ASSIGNMENT: &str =
	"[Work: held-work; Requirements: r1; Contribution: held-child]\nHELD_CHILD_RESULT";

fn contribution() -> NativeContribution {
	NativeContribution {
		id: "held-child".into(),
		work_id: "held-work".into(),
		requirements_id: "r1".into(),
		instruction: "HELD_CHILD_RESULT".into(),
		child_thread_id: None,
		parent_turn_id: None,
		parent_item_id: None,
	}
}

fn is_child(body: &Value) -> bool {
	body["client_metadata"]["x-openai-subagent"].as_str().is_some_and(|role| role != "guardian")
}

fn response(body: &Value, serial: usize, v2: bool, nested: bool) -> Value {
	if !v2 && serial == 1 {
		return serde_json::json!({"type":"tool_search_call","call_id":"discover-native-spawn","execution":"client","arguments":{"query":"spawn agent","limit":1}});
	}
	if serial == if v2 { 1 } else { 2 } {
		let arguments = if v2 {
			serde_json::json!({"task_name":contribution().native_task_name(),"message":NATIVE_ASSIGNMENT,"fork_turns":"none"})
		} else {
			serde_json::json!({"message":NATIVE_ASSIGNMENT})
		};
		if nested {
			let namespace = if v2 { "collaboration" } else { "multi_agent_v1" };
			return serde_json::json!({"type":"custom_tool_call","name":"exec","call_id":"spawn-cell","input":format!("text(await tools.{namespace}__spawn_agent({arguments}));")});
		}
		return serde_json::json!({"type":"function_call","id":"spawn-child","call_id":"spawn-child","namespace":if v2{"collaboration"}else{"multi_agent_v1"},"name":"spawn_agent","arguments":arguments.to_string()});
	}

	let input = body["input"].as_array().expect("input");
	let child = is_child(body);
	let answered = input
		.iter()
		.any(|item| item["type"] == "function_call_output" && item["call_id"] == "held-command");

	if child && !answered {
		return serde_json::json!({"type":"function_call","id":"held-command","call_id":"held-command","namespace":"functions","name":"exec_command","arguments":serde_json::json!({"cmd":"printf unused-fixture","sandbox_permissions":"require_escalated","justification":"Hold the isolated child until its root is idle"}).to_string()});
	}

	let text = if child {
		"CHILD_RESULT_READY"
	} else if input.iter().any(|item| item["type"] == "agent_message") {
		"ROOT_CONTINUED"
	} else {
		"ROOT_DONE"
	};

	serde_json::json!({"type":"message","role":"assistant","phase":"final_answer","id":format!("message-{serial}"),"content":[{"type":"output_text","text":text}]})
}

async fn serve(
	listener: TcpListener,
	requests: mpsc::UnboundedSender<Value>,
	v2: bool,
	nested: bool,
) {
	let mut serial = 0;

	while let Ok((mut socket, _)) = listener.accept().await {
		serial += 1;

		let body = native_task_references::read_http_body(&mut socket).await;
		let item = response(&body, serial, v2, nested);

		requests.send(body).expect("request observation");

		let id = format!("wake-fixture-{serial}");
		let frames = [
			serde_json::json!({"type":"response.created","response":{"id":id}}),
			serde_json::json!({"type":"response.output_item.done","item":item}),
			serde_json::json!({"type":"response.completed","response":{"id":id,"usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0}}}),
		];
		let data = frames
			.iter()
			.map(|frame| format!("event: {}\ndata: {frame}\n\n", frame["type"].as_str().unwrap()))
			.collect::<String>();
		let http = format!(
			"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}",
			data.len()
		);

		socket.write_all(http.as_bytes()).await.expect("fixture response");
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY and its packaged helpers; isolated loopback provider"]
async fn native_child_completion_wakes_idle_root_once() {
	for enabled in [false, true] {
		qualify(enabled, true, false).await;
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY and packaged helpers; isolated native V1 Work attribution"]
async fn native_v1_work_attribution_uses_the_native_executor() {
	qualify(false, false, false).await;
}

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY and helpers; isolated nested native V2 Work attribution"]
async fn native_nested_code_mode_work_attribution_survives_store_reopen() {
	qualify(false, true, true).await;
}

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY and helpers; isolated nested native V1 Work attribution"]
async fn native_v1_nested_code_mode_keeps_exact_work_attribution() {
	qualify(false, false, true).await;
}

async fn qualify(enabled: bool, v2: bool, nested: bool) {
	let binary = env::var("DECODEX_NATIVE_BINARY").expect("exact native binary");
	let directory = tempfile::tempdir().expect("fixture home");
	let home = directory.path().canonicalize().expect("canonical home");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("fixture listener");
	let address = listener.local_addr().expect("fixture address");
	let (requests_tx, mut requests_rx) = mpsc::unbounded_channel();
	let backend = tokio::spawn(serve(listener, requests_tx, v2, nested));
	// The native model catalog selects the backend. Disabling the V2 feature does
	// not override a model that already selects V2.
	let model = if v2 { "gpt-5.6-sol" } else { "gpt-5.6-luna" };

	// Native V2 hides collaboration from Code Mode by default. Exercise nested
	// execution only in the explicit native configuration that exposes it.
	let v2_setting = if v2 && nested {
		"{ enabled = true, non_code_mode_only = false }".to_owned()
	} else {
		v2.to_string()
	};
	fs::write(home.join("config.toml"),format!("model=\"{model}\"\nmodel_provider=\"fixture\"\nservice_tier=\"priority\"\napprovals_reviewer=\"user\"\n[features]\nmulti_agent=true\nmulti_agent_v2={v2_setting}\ncode_mode={nested}\n[model_providers.fixture]\nname=\"Isolated root wake fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("fixture config");

	let (mut agent, _, store_home) = tests::fixture().await;
	let mut command = native_command(&binary, &home, enabled);
	let (client, mut events, mut process) =
		AppServerClient::spawn(&mut command).expect("native process");

	agent.client = client;
	agent.config = AgentConfig::new(model.into(), "medium".into(), home.display().to_string());
	agent.config.sandbox = "read-only".into();

	let outcome = AssertUnwindSafe(time::timeout(Duration::from_secs(60), async {
		agent.initialize().await.expect("initialize");
		AgentCoordinator::reserve_root(&agent.store,"agent","ROOT_SPAWN").await.unwrap();
		prepare_work(&agent).await;
		agent.start_reserved_agent("agent", "ROOT_SPAWN").await.expect("start root");

		let initial = agent.store.get_agent_work_item("agent".into()).await.unwrap();
		let root = initial.codex_thread_id.unwrap();
		let initial_turn = initial.active_turn_id.unwrap();


		let mut root_idle = false;
		let mut pending_child = None;
		let mut released_child = None;
		let mut child_completed = false;
		let mut wake_turn = None;

		loop {
			let event = events.recv().await.expect("native event stream");
			let mut root_started = None;
			let mut root_completed = None;

			match &event {
				ServerEvent::Request { id, method, params }
					if method == "item/commandExecution/requestApproval" =>
				{
					assert_ne!(params["threadId"], root);

					pending_child = Some((id.clone(), params["threadId"].as_str().unwrap().to_owned()));
				},
				ServerEvent::Notification { method, params } if method == "turn/started" && params["threadId"] == root => {
					root_started = Some(params["turn"]["id"].as_str().unwrap().to_owned());
				},
				ServerEvent::Notification { method, params } if method == "turn/completed" => {
					if params["threadId"] == root {
						root_completed = Some(params["turn"]["id"].as_str().unwrap().to_owned());
					} else if params["threadId"].as_str() == released_child.as_deref() {
						child_completed = true;
					}
				},
				_ => {},
			}

			agent.handle_event(event).await.expect("native event projection");

			if let Some(turn) = root_started.filter(|turn| turn != &initial_turn) {
				assert!(enabled && root_idle, "only a completed root can wake");
				assert!(wake_turn.replace(turn.clone()).is_none(), "one native continuation");

				let work = agent.store.get_agent_work_item("agent".into()).await.unwrap();

				assert_eq!(work.dispatch_state, AgentDispatchState::Running);
				assert_eq!(work.active_turn_id.as_deref(), Some(turn.as_str()));
			}
			if let Some(turn) = root_completed {
				let work = agent.store.get_agent_work_item("agent".into()).await.unwrap();

				assert_eq!(work.dispatch_state, AgentDispatchState::Idle);
				assert!(work.active_turn_id.is_none());
				assert!(agent.store.agent_native_terminal_recorded("agent".into(), root.clone(), turn.clone()).await.unwrap());
				if turn != initial_turn { break; }

				root_idle = true;
			}
			if root_idle && let Some((id, child)) = pending_child.take() {
				let inbox = agent.pending_requests[&id];

				// No command executes. The withheld approval is a deterministic child-result gate.
				agent.respond_pending_event(inbox, serde_json::json!({"decision":"decline"})).await.unwrap();

				released_child = Some(child);
			}
			if !enabled && child_completed { break; }
		}

		assert!(root_idle && released_child.is_some());
		assert_eq!(wake_turn.is_some(), enabled);
		if nested {
			qualify_reopened_store(&agent, store_home.path(), v2).await;
		} else {
			qualify_work_result(&mut agent).await;
		}

		// Recovery and the local due-work tick must not replay either completed root turn.
		agent.recover_native_turns().await.unwrap();
		agent.wake_pending().await.unwrap();

		let quiet = time::sleep(Duration::from_millis(350));

		tokio::pin!(quiet);
		loop {
			tokio::select! {
				() = &mut quiet => break,
				event = events.recv() => {
					let event = event.expect("native event stream");
					assert!(!matches!(&event, ServerEvent::Notification{method,params} if method=="turn/started" && params["threadId"]==root), "no duplicate or disabled wake");
					agent.handle_event(event).await.unwrap();
				},
			}
		}

		let requests: Vec<_> = std::iter::from_fn(|| requests_rx.try_recv().ok()).collect();
		assert_requests(&requests, enabled, v2);
	})).catch_unwind().await;

	process.shutdown().await.expect("shutdown owned native process");
	backend.abort();
	outcome.expect("native wake qualification panicked").expect("native wake deadline");
}

fn assert_requests(requests: &[Value], enabled: bool, v2: bool) {
	let roots: Vec<_> = requests.iter().filter(|body| !is_child(body)).collect();
	let discovery = usize::from(!v2);
	assert_eq!(requests.len(), if enabled { 5 } else { 4 } + discovery);
	assert_eq!(roots.len(), if enabled { 3 } else { 2 } + discovery);
	if enabled {
		let continuation = roots.last().unwrap();
		let input = continuation["input"].as_array().unwrap();
		assert_eq!(continuation["model"], "gpt-5.6-sol");
		assert_eq!(continuation["reasoning"]["effort"], "medium");
		assert_eq!(continuation["service_tier"], "priority");
		assert_eq!(input.iter().filter(|item| item["type"] == "agent_message").count(), 1);
		assert!(continuation.to_string().contains("CHILD_RESULT_READY"));
		assert!(continuation.to_string().contains("ROOT_DONE"), "prior root history survives");
	}
}

async fn qualify_reopened_store(agent: &AgentCoordinator, home: &std::path::Path, v2: bool) {
	let root = decodex_core::DecodexRoot::new(home.canonicalize().unwrap().join("root")).unwrap();
	if v2 {
		assert!(agent.store.native_work_delivery("held-child".into()).await.unwrap().is_some());
		// Reconstruct the durable crash point after input was observed, before its
		// parent call and child binding were committed. The actual native history stays intact.
		let c = rusqlite::Connection::open(root.paths().product_database_file()).unwrap();
		c.execute_batch("BEGIN; DELETE FROM native_work_deliveries WHERE contribution_id='held-child'; DELETE FROM native_work_dispatches WHERE contribution_id='held-child'; DELETE FROM work_commands WHERE id='native-binding:held-child'; UPDATE native_work_contributions SET child_thread_id=NULL,parent_turn_id=NULL,parent_item_id=NULL WHERE id='held-child'; COMMIT;").unwrap();
	}
	let reopened = decodex_database::SqliteStore::open(&root.paths()).unwrap();
	let mut recovered =
		AgentCoordinator::new(reopened, agent.client.clone(), agent.config.clone()).unwrap();
	qualify_work_result(&mut recovered).await;
}

async fn prepare_work(agent: &AgentCoordinator) {
	let work = Work {
		id: "held-work".into(),
		project_id: None,
		owner_agent_id: "agent".into(),
		title: "Inspect the held native contribution".into(),
		requirements: WorkRequirements {
			id: "r1".into(),
			goal: "Return the isolated child result".into(),
			criteria: "Exact native assignment and Result, no unrelated completion".into(),
			source: "Synthetic user qualification".into(),
		},
		state: WorkState::Open,
		reason: None,
		resume_condition: None,
		user_hold: false,
		assessment: None,
	};
	agent
		.store
		.apply_work_command(
			"agent".into(),
			"native-work".into(),
			WorkCommand::Create { work },
			true,
		)
		.await
		.unwrap();
	agent
		.store
		.apply_work_command(
			"agent".into(),
			"prepare-child".into(),
			WorkCommand::PrepareSubagent { contribution: contribution() },
			true,
		)
		.await
		.unwrap();
}

async fn qualify_work_result(agent: &mut AgentCoordinator) {
	let current = agent.store.get_agent_work_item("agent".into()).await.unwrap();
	let sources = agent
		.work_tool(&current, "work_read", &serde_json::json!({"nativeContributionId":"held-child"}))
		.await
		.unwrap();
	let output = sources["outputs"]
		.as_array()
		.unwrap()
		.iter()
		.find(|o| o["kind"] == "agentMessage")
		.expect("exact assigned child turn must remain readable");
	let result = WorkResult {
		id: "child-result".into(),
		work_id: "held-work".into(),
		requirements_id: "r1".into(),
		instruction_id: None,
		native_contribution_id: Some("held-child".into()),
		agent_id: "agent".into(),
		summary: "Native child returned".into(),
		source: ResultSource::NativeItem {
			thread_id: output["threadId"].as_str().unwrap().into(),
			turn_id: output["turnId"].as_str().unwrap().into(),
			item_id: output["itemId"].as_str().unwrap().into(),
		},
		limits: "Loopback fixture; no model quality claim".into(),
	};
	agent.verify_work_result("agent", &result).await.unwrap();
	agent
		.store
		.apply_work_command(
			"agent".into(),
			"child-result".into(),
			WorkCommand::RecordResult { result },
			true,
		)
		.await
		.unwrap();
	assert_eq!(
		agent.store.work_portfolio(Some("held-work".into())).await.unwrap().works[0].state,
		WorkState::Open,
		"Native completion and returned Result do not complete Work"
	);
}

fn native_command(binary: &str, home: &std::path::Path, enabled: bool) -> Command {
	let mut command = Command::new(binary);

	command
		.args([
			"app-server",
			if enabled { "--enable" } else { "--disable" },
			"wake_on_agent_messages",
		])
		.current_dir(home)
		.env_clear()
		.env("HOME", home)
		.env("CODEX_HOME", home)
		.env("PATH", "/usr/bin:/bin");

	command
}
