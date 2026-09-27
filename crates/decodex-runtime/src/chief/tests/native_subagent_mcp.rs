//! Native child human-input requests stop before creating local pending prompts.
use super::*;

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY; isolated child MCP input"]
async fn native_child_mcp_input_returns_handoff_without_local_prompt() {
	let mut failures = 0;
	for marker in
		[json!({"codex_approval_kind":"browser_auth"}), json!({"codex_requires_user_input":true})]
	{
		if std::panic::AssertUnwindSafe(qualify(marker)).catch_unwind().await.is_err() {
			failures += 1;
		}
	}
	assert_eq!(failures, 0, "native child input markers must both require root handoff");
}

async fn qualify(marker: Value) {
	let binary = std::env::var("DECODEX_NATIVE_BINARY").expect("native binary");
	let directory = tempfile::tempdir().expect("fixture home");
	let home = directory.path().canonicalize().expect("canonical home");
	let script = home.join("mcp.py");
	std::fs::write(&script, include_str!("native_subagent_mcp_server.py")).expect("fixture script");
	let record = home.join("result.json");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
	let address = listener.local_addr().expect("address");
	let (requests_tx, mut requests_rx) = tokio::sync::mpsc::unbounded_channel();
	let backend = tokio::spawn(serve_with_response(listener, requests_tx, mcp_response));
	std::fs::write(home.join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nmulti_agent=true\nmulti_agent_v2=true\n[model_providers.fixture]\nname=\"Isolated MCP child\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n[mcp_servers.fixture]\ncommand=\"/usr/bin/python3\"\nargs=[{},{},{}]\nrequired=true\ndefault_tools_approval_mode=\"approve\"\n",json!(script),json!(record),json!(marker.to_string()))).expect("config");
	let (mut chief, _, _store_home) = fixture().await;
	let mut command = tokio::process::Command::new(binary);
	command
		.arg("app-server")
		.current_dir(&home)
		.env_clear()
		.env("HOME", &home)
		.env("CODEX_HOME", &home)
		.env("PATH", "/usr/bin:/bin");
	let (client, mut events, mut process) =
		AppServerClient::spawn(&mut command).expect("native process");
	chief.client = client;
	chief.config =
		ChiefConfig::new("gpt-5.6-sol".into(), "medium".into(), home.display().to_string());
	chief.config.sandbox = "danger-full-access".into();
	chief.config.approval_policy = json!("never");
	let outcome=std::panic::AssertUnwindSafe(tokio::time::timeout(std::time::Duration::from_secs(60),async{
 chief.initialize().await.expect("initialize");
 chief.start_chief("chief","ROOT_SPAWN").await.expect("start root");
 let root=chief.store.get_chief_work_item("chief".into()).await.expect("root").codex_thread_id.expect("root thread");
 loop {
  let event=events.recv().await.expect("native event");
  assert!(!matches!(&event,ServerEvent::Request{method,..} if method=="mcpServer/elicitation/request"),"child human input reached local UI");
  let done=matches!(&event,ServerEvent::Notification{method,params} if method=="turn/completed" && params["threadId"]!=root);
  chief.handle_event(event).await.expect("Chief event");
  if done {break;}
 }
 assert!(chief.pending_requests.is_empty(),"no local prompt registered");
 let result:Value=serde_json::from_slice(&std::fs::read(&record).expect("native MCP result")).expect("result JSON");
 assert_eq!(result["error"]["code"],-32603,"marker={marker}, native reply={result}");
 assert!(result["error"]["message"].as_str().expect("handoff message").contains("root thread"));
 let mut child_calls=0;
 let mut handoffs=0;
 while let Ok(body)=requests_rx.try_recv() {
  if is_child(&body) {
   let output=body["input"].as_array().expect("child input").iter().find(|i|i["type"]=="function_call_output" && i["call_id"]=="child-mcp");
   if let Some(output)=output {assert!(output.to_string().contains("root thread"));handoffs+=1;} else {child_calls+=1;}
  }
 }
 assert_eq!(child_calls,1,"no automatic MCP retry");
 assert_eq!(handoffs,1,"exact child receives handoff guidance");
 })).catch_unwind().await;
	process.shutdown().await.expect("shutdown");
	backend.abort();
	outcome.expect("child MCP qualification").expect("child MCP deadline");
}

fn mcp_response(body: &Value, serial: usize) -> Value {
	if serial == 1 {
		return response(body, serial);
	}
	let answered = body["input"]
		.as_array()
		.expect("input")
		.iter()
		.any(|i| i["type"] == "function_call_output" && i["call_id"] == "child-mcp");
	if is_child(body) && !answered {
		return json!({"type":"function_call","id":"child-mcp","call_id":"child-mcp","namespace":"mcp__fixture","name":"ask","arguments":"{}"});
	}
	json!({"type":"message","role":"assistant","id":format!("done-{serial}"),"content":[{"type":"output_text","text":"DONE"}]})
}
