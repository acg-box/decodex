//! A native child result resumes an idle root through the ordinary event bridge.
use std::{env, fs, panic::AssertUnwindSafe, time::Duration};

use futures_util::FutureExt as _;
use tokio::{net::TcpListener, process::Command, sync::mpsc, time};

use crate::agent::{
	tests,
	tests::{
		AgentConfig, AppServerClient, AsyncWriteExt, ServerEvent, Value, native_task_references,
	},
};
use decodex_database::AgentDispatchState;

fn is_child(body: &Value) -> bool {
	body.to_string().contains("HELD_CHILD_RESULT")
		&& !body["input"]
			.as_array()
			.expect("input")
			.iter()
			.any(|item| item["type"] == "function_call" && item["call_id"] == "spawn-child")
}

fn response(body: &Value, serial: usize) -> Value {
	if serial == 1 {
		return serde_json::json!({"type":"function_call","id":"spawn-child","call_id":"spawn-child","namespace":"collaboration","name":"spawn_agent","arguments":serde_json::json!({"task_name":"worker","message":"HELD_CHILD_RESULT","fork_turns":"none"}).to_string()});
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

async fn serve(listener: TcpListener, requests: mpsc::UnboundedSender<Value>) {
	let mut serial = 0;

	while let Ok((mut socket, _)) = listener.accept().await {
		serial += 1;

		let body = native_task_references::read_http_body(&mut socket).await;
		let item = response(&body, serial);

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
		qualify(enabled).await;
	}
}

async fn qualify(enabled: bool) {
	let binary = env::var("DECODEX_NATIVE_BINARY").expect("exact native binary");
	let directory = tempfile::tempdir().expect("fixture home");
	let home = directory.path().canonicalize().expect("canonical home");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("fixture listener");
	let address = listener.local_addr().expect("fixture address");
	let (requests_tx, mut requests_rx) = mpsc::unbounded_channel();
	let backend = tokio::spawn(serve(listener, requests_tx));

	fs::write(home.join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\nservice_tier=\"priority\"\napprovals_reviewer=\"user\"\n[features]\nmulti_agent=true\nmulti_agent_v2=true\n[model_providers.fixture]\nname=\"Isolated root wake fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("fixture config");

	let (mut agent, _, _store_home) = tests::fixture().await;
	let mut command = Command::new(binary);

	command
		.args([
			"app-server",
			if enabled { "--enable" } else { "--disable" },
			"wake_on_agent_messages",
		])
		.current_dir(&home)
		.env_clear()
		.env("HOME", &home)
		.env("CODEX_HOME", &home)
		.env("PATH", "/usr/bin:/bin");

	let (client, mut events, mut process) =
		AppServerClient::spawn(&mut command).expect("native process");

	agent.client = client;
	agent.config =
		AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), home.display().to_string());
	agent.config.sandbox = "read-only".into();

	let outcome = AssertUnwindSafe(time::timeout(Duration::from_secs(60), async {
		agent.initialize().await.expect("initialize");
		agent.start_agent("agent", "ROOT_SPAWN").await.expect("start root");

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
		let roots: Vec<_> = requests.iter().filter(|body| !is_child(body)).collect();

		assert_eq!(requests.len(), if enabled { 5 } else { 4 });
		assert_eq!(roots.len(), if enabled { 3 } else { 2 });
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
	})).catch_unwind().await;

	process.shutdown().await.expect("shutdown owned native process");
	backend.abort();
	outcome.expect("native wake qualification panicked").expect("native wake deadline");
}
