//! Exercise child request routing through the real native app-server.
#[path = "native_subagent_mcp.rs"] mod mcp;

use std::{env, fs, panic::AssertUnwindSafe, time::Duration};

use futures_util::FutureExt as _;
use tokio::{
	process::Command,
	sync::{mpsc, mpsc::UnboundedSender},
	time,
};

use crate::{
	agent::tests::{
		AgentConfig, AppServerClient, AsyncWriteExt, ServerEvent, Value, fixture,
		native_task_references,
	},
	native_agents,
};
use decodex_protocol::NativeAgentsResult;

fn response(body: &Value, serial: usize) -> Value {
	if serial == 1 {
		return serde_json::json!({"type":"function_call","id":"spawn","call_id":"spawn","namespace":"collaboration","name":"spawn_agent","arguments":serde_json::json!({"task_name":"child","message":"CHILD_APPROVAL","fork_turns":"none"}).to_string()});
	}

	let input = body["input"].as_array().unwrap();
	let child = is_child(body);
	let answered = input
		.iter()
		.any(|v| v["type"] == "function_call_output" && v["call_id"] == "child-command");

	if child && !answered {
		return serde_json::json!({"type":"function_call","id":"child-command","call_id":"child-command","namespace":"functions","name":"exec_command","arguments":serde_json::json!({"cmd":"printf child-fixture","sandbox_permissions":"require_escalated","justification":"Isolated approval fixture"}).to_string()});
	}

	serde_json::json!({"type":"message","role":"assistant","id":format!("message-{serial}"),"content":[{"type":"output_text","text":"DONE"}]})
}

fn is_child(body: &Value) -> bool {
	body.to_string().contains("CHILD_APPROVAL")
		&& !body["input"]
			.as_array()
			.unwrap()
			.iter()
			.any(|v| v["type"] == "function_call" && v["call_id"] == "spawn")
}

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY pointing to an installed Codex binary"]
async fn native_child_approval_round_trip() {
	let binary = env::var("DECODEX_NATIVE_BINARY").unwrap();
	let directory = tempfile::tempdir().unwrap();
	let home = directory.path().canonicalize().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let (requests_tx, mut requests_rx) = mpsc::unbounded_channel();
	let backend = tokio::spawn(serve(listener, requests_tx));

	fs::write(home.join("config.toml"),format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\nservice_tier = \"priority\"\napprovals_reviewer = \"user\"\n[features]\nmulti_agent = true\nmulti_agent_v2 = true\n[model_providers.fixture]\nname = \"Isolated child approval fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();

	let (mut agent, _sent, _store_home) = fixture().await;
	let mut command = Command::new(binary);

	command
		.arg("app-server")
		.current_dir(&home)
		.env_clear()
		.env("HOME", &home)
		.env("CODEX_HOME", &home)
		.env("PATH", "/usr/bin:/bin");

	let (client, mut events, mut process) = AppServerClient::spawn(&mut command).unwrap();

	agent.client = client;
	agent.config =
		AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), home.display().to_string());
	agent.config.sandbox = "read-only".into();

	let outcome = AssertUnwindSafe(time::timeout(Duration::from_secs(60), async {
		agent.initialize().await.unwrap();
		agent.start_agent("agent", "ROOT_SPAWN").await.unwrap();

		let root = agent.store.get_agent_work_item("agent".into()).await.unwrap().codex_thread_id.unwrap();
		let mut approved_child = None;
		let mut event_id = None;

		loop {
			let event = events.recv().await.expect("native event stream ended");
			let request = match &event {
				ServerEvent::Request {id,method,params} if method == "item/commandExecution/requestApproval" => Some((id.clone(),params["threadId"].as_str().unwrap().to_owned())),
				_ => None,
			};
			let completed = matches!(&event,ServerEvent::Notification {method,params} if method=="turn/completed" && params["threadId"].as_str()==approved_child.as_deref() && approved_child.is_some());

			agent.handle_event(event).await.unwrap();

			if let Some((id, child)) = request {
				assert_ne!(child,root);

                let listed=native_agents::read(&agent.store,&agent.client,"agent",None,None).await;

                assert!(matches!(&listed,NativeAgentsResult::Available{agents,..} if agents.iter().any(|agent|agent.thread_id==child && agent.parent_thread_id==root)), "native descendant missing: {listed:?}");

                let inspected=native_agents::read(&agent.store,&agent.client,"agent",Some(&child),None).await;

                assert!(matches!(inspected,NativeAgentsResult::Conversation{can_input:false,..}), "native v2 input capability was not preserved: {inspected:?}");

				let pending = agent.pending_requests[&id];
				let saved = agent.store.get_agent_inbox_event(pending).await.unwrap();

				assert_eq!(saved.work_item_id,"agent");

				let payload: Value = serde_json::from_str(&saved.payload).unwrap();

				assert_eq!(payload["params"]["threadId"],child);

				// A root setting update must reach the next step of the already running child.
				agent.client.request("thread/settings/update",serde_json::json!({"threadId":root,"serviceTier":null})).await.unwrap();
				agent.respond_pending_event(pending,serde_json::json!({"decision":"decline"})).await.unwrap();

				approved_child = Some(child);
				event_id = Some(pending);
			}

			if completed {break;}
		}

		let saved = agent.store.get_agent_inbox_event(event_id.unwrap()).await.unwrap();

		assert!(saved.disposition.is_some());

		let mut request_count = 0;
		let mut initial_child = false;
		let mut continued_child = false;

		while let Ok(body) = requests_rx.try_recv() {
			if request_count == 0 { assert_eq!(body["service_tier"], "priority"); }
			if is_child(&body) {
                assert_eq!(body["model"], "gpt-5.6-sol", "child keeps the invoking model");
                assert_eq!(body["reasoning"]["effort"], "medium", "child keeps the invoking effort");

				let resumed = body["input"].as_array().unwrap().iter().any(|v| v["type"] == "function_call_output" && v["call_id"] == "child-command");

				if resumed {
					assert!(body["service_tier"].is_null(), "existing child must follow the cleared root tier");

					continued_child = true;
				} else {
					assert_eq!(body["service_tier"], "priority", "new child must inherit root tier");

					initial_child = true;
				}
			}

			request_count += 1;
		}

		assert!(request_count >= 3, "root spawn and child approval continuation reached the backend");
		assert!(initial_child && continued_child);
		assert_eq!(agent.store.get_agent_work_item("agent".into()).await.unwrap().codex_thread_id.as_deref(),Some(root.as_str()));
	})).catch_unwind().await;

	process.shutdown().await.unwrap();
	backend.abort();
	outcome.expect("native child approval panicked").expect("native child approval timed out");
}

async fn serve(listener: tokio::net::TcpListener, requests: UnboundedSender<Value>) {
	serve_with_response(listener, requests, response).await;
}

async fn serve_with_response(
	listener: tokio::net::TcpListener,
	requests: UnboundedSender<Value>,
	response_fn: fn(&Value, usize) -> Value,
) {
	let mut serial = 0;

	while let Ok((mut socket, _)) = listener.accept().await {
		serial += 1;

		let body = native_task_references::read_http_body(&mut socket).await;

		requests.send(body.clone()).unwrap();

		let item = response_fn(&body, serial);
		let id = format!("fixture-{serial}");
		let frames = [
			serde_json::json!({"type":"response.created","response":{"id":id}}),
			serde_json::json!({"type":"response.output_item.done","item":item}),
			serde_json::json!({"type":"response.completed","response":{"id":id,"usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0}}}),
		];
		let data = frames
			.iter()
			.map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap()))
			.collect::<String>();
		let response = format!(
			"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}",
			data.len()
		);

		socket.write_all(response.as_bytes()).await.unwrap();
	}
}
