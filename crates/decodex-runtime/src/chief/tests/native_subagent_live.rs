//! Exercise child request routing through the real native app-server.
use super::*;
use futures_util::FutureExt as _;

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY pointing to an installed Codex binary"]
async fn native_child_approval_round_trip() {
	let binary = std::env::var("DECODEX_NATIVE_BINARY").unwrap();
	let directory = tempfile::tempdir().unwrap();
	let home = directory.path().canonicalize().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let backend = tokio::spawn(serve(listener));
	std::fs::write(home.join("config.toml"),format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\napprovals_reviewer = \"user\"\n[features]\nmulti_agent = true\nmulti_agent_v2 = true\n[model_providers.fixture]\nname = \"Isolated child approval fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();
	let (mut chief, _sent, _store_home) = fixture().await;
	let mut command = tokio::process::Command::new(binary);
	command
		.arg("app-server")
		.current_dir(&home)
		.env_clear()
		.env("HOME", &home)
		.env("CODEX_HOME", &home)
		.env("PATH", "/usr/bin:/bin");
	let (client, mut events, mut process) = AppServerClient::spawn(&mut command).unwrap();
	chief.client = client;
	chief.config =
		ChiefConfig::new("gpt-5.6-sol".into(), "medium".into(), home.display().to_string());
	chief.config.sandbox = "read-only".into();
	let outcome = std::panic::AssertUnwindSafe(tokio::time::timeout(std::time::Duration::from_secs(60), async {
		chief.initialize().await.unwrap();
		chief.start_chief("chief", "ROOT_SPAWN").await.unwrap();
		let root = chief.store.get_chief_work_item("chief".into()).await.unwrap().codex_thread_id.unwrap();
		let mut approved_child = None;
		let mut event_id = None;
		loop {
			let event = events.recv().await.expect("native event stream ended");

			let request = match &event {
				ServerEvent::Request {id,method,params} if method == "item/commandExecution/requestApproval" => Some((id.clone(),params["threadId"].as_str().unwrap().to_owned())),
				_ => None,
			};
			let completed = matches!(&event,ServerEvent::Notification {method,params} if method=="turn/completed" && params["threadId"].as_str()==approved_child.as_deref() && approved_child.is_some());
			chief.handle_event(event).await.unwrap();
			if let Some((id, child)) = request {
				assert_ne!(child,root);
                let listed=crate::native_agents::read(&chief.store,&chief.client,"chief",None,None).await;
                assert!(matches!(&listed,decodex_protocol::NativeAgentsResult::Available{agents,..} if agents.iter().any(|agent|agent.thread_id==child && agent.parent_thread_id==root)), "native descendant missing: {listed:?}");
                let inspected=crate::native_agents::read(&chief.store,&chief.client,"chief",Some(&child),None).await;
                assert!(matches!(inspected,decodex_protocol::NativeAgentsResult::Conversation{can_input:false,..}), "native v2 input capability was not preserved: {inspected:?}");
				let pending = chief.pending_requests[&id];
				let saved = chief.store.get_chief_inbox_event(pending).await.unwrap();
				assert_eq!(saved.work_item_id,"chief");
				let payload: Value = serde_json::from_str(&saved.payload).unwrap();
				assert_eq!(payload["params"]["threadId"],child);
				chief.respond_pending_event(pending,json!({"decision":"decline"})).await.unwrap();
				approved_child = Some(child);
				event_id = Some(pending);
			}
			if completed {break;}
		}
		let saved = chief.store.get_chief_inbox_event(event_id.unwrap()).await.unwrap();
		assert!(saved.disposition.is_some());
		assert_eq!(chief.store.get_chief_work_item("chief".into()).await.unwrap().codex_thread_id.as_deref(),Some(root.as_str()));
	})).catch_unwind().await;
	process.shutdown().await.unwrap();
	backend.abort();
	outcome.expect("native child approval panicked").expect("native child approval timed out");
}

async fn serve(listener: tokio::net::TcpListener) {
	let mut serial = 0;
	while let Ok((mut socket, _)) = listener.accept().await {
		serial += 1;
		let body = native_task_references::read_http_body(&mut socket).await;
		let item = response(&body, serial);
		let id = format!("fixture-{serial}");
		let frames = [
			json!({"type":"response.created","response":{"id":id}}),
			json!({"type":"response.output_item.done","item":item}),
			json!({"type":"response.completed","response":{"id":id,"usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0}}}),
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

fn response(body: &Value, serial: usize) -> Value {
	if serial == 1 {
		return json!({"type":"function_call","id":"spawn","call_id":"spawn","namespace":"collaboration","name":"spawn_agent","arguments":json!({"task_name":"child","message":"CHILD_APPROVAL","fork_turns":"none"}).to_string()});
	}
	let input = body["input"].as_array().unwrap();
	let child = body.to_string().contains("CHILD_APPROVAL")
		&& !input.iter().any(|v| v["type"] == "function_call" && v["call_id"] == "spawn");
	let answered = input
		.iter()
		.any(|v| v["type"] == "function_call_output" && v["call_id"] == "child-command");
	if child && !answered {
		return json!({"type":"function_call","id":"child-command","call_id":"child-command","namespace":"functions","name":"exec_command","arguments":json!({"cmd":"printf child-fixture","sandbox_permissions":"require_escalated","justification":"Isolated approval fixture"}).to_string()});
	}
	json!({"type":"message","role":"assistant","id":format!("message-{serial}"),"content":[{"type":"output_text","text":"DONE"}]})
}
