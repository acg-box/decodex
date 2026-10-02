//! Native child MCP requests retain policy decisions and exact response ownership.
use std::{env, fs, panic::AssertUnwindSafe, time::Duration};

use futures_util::FutureExt as _;
use tokio::{net::TcpListener, process::Command, sync::mpsc, time};

use crate::agent::{
	tests,
	tests::native_subagent_live::{self, AgentConfig, AppServerClient, ServerEvent, Value},
};

fn mcp_response(body: &Value, serial: usize) -> Value {
	if serial == 1 {
		return native_subagent_live::response(body, serial);
	}

	let answered = body["input"]
		.as_array()
		.expect("input")
		.iter()
		.any(|i| i["type"] == "function_call_output" && i["call_id"] == "child-mcp");

	if native_subagent_live::is_child(body) && !answered {
		return serde_json::json!({"type":"function_call","id":"child-mcp","call_id":"child-mcp","namespace":"mcp__fixture","name":"ask","arguments":"{}"});
	}

	serde_json::json!({"type":"message","role":"assistant","id":format!("done-{serial}"),"content":[{"type":"output_text","text":"DONE"}]})
}

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY; isolated child MCP input"]
async fn native_child_mcp_input_preserves_policy_and_exact_request() {
	let mut failures = 0;

	for marker in [
		serde_json::json!({"codex_approval_kind":"browser_auth"}),
		serde_json::json!({"codex_requires_user_input":true}),
	] {
		for interactive in [false, true] {
			if AssertUnwindSafe(qualify(marker.clone(), interactive)).catch_unwind().await.is_err()
			{
				failures += 1;
			}
		}
	}

	assert_eq!(failures, 0, "native child input must preserve policy and request ownership");
}

async fn qualify(marker: Value, interactive: bool) {
	let binary = env::var("DECODEX_NATIVE_BINARY").expect("native binary");
	let directory = tempfile::tempdir().expect("fixture home");
	let home = directory.path().canonicalize().expect("canonical home");
	let script = home.join("mcp.py");

	fs::write(&script, include_str!("native_subagent_mcp_server.py")).expect("fixture script");

	let record = home.join("result.json");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
	let address = listener.local_addr().expect("address");
	let (requests_tx, mut requests_rx) = mpsc::unbounded_channel();
	let backend = tokio::spawn(native_subagent_live::serve_with_response(
		listener,
		requests_tx,
		mcp_response,
	));

	fs::write(home.join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\napprovals_reviewer=\"user\"\n[features]\nmulti_agent=true\nmulti_agent_v2=true\n[model_providers.fixture]\nname=\"Isolated MCP child\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n[mcp_servers.fixture]\ncommand=\"/usr/bin/python3\"\nargs=[{},{},{},{}]\nrequired=true\ndefault_tools_approval_mode=\"approve\"\n",serde_json::json!(script),serde_json::json!(record),serde_json::json!(marker.to_string()),serde_json::json!(interactive.to_string()))).expect("config");

	let (mut agent, _, _store_home) = tests::fixture().await;
	let mut command = Command::new(binary);

	command
		.arg("app-server")
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
	agent.config.sandbox = if interactive { "read-only" } else { "danger-full-access" }.into();
	agent.config.approval_policy =
		serde_json::json!(if interactive { "on-request" } else { "never" });

	let outcome=AssertUnwindSafe(time::timeout(Duration::from_secs(60),async{
 agent.initialize().await.expect("initialize");
 agent.start_agent("agent","ROOT_SPAWN").await.expect("start root");

 let root=agent.store.get_agent_work_item("agent".into()).await.expect("root").codex_thread_id.expect("root thread");
 let mut prompts=0;

 loop {
  let event=events.recv().await.expect("native event");
  let request = match &event {
   ServerEvent::Request{id,method,params} if method=="mcpServer/elicitation/request" => {
    assert!(interactive,"native automatic decision must not prompt");
    assert_ne!(params["threadId"],root,"prompt belongs to native child");

    Some((id.clone(),params.clone()))
   },
   _ => None,
  };
  let done=matches!(&event,ServerEvent::Notification{method,params} if method=="turn/completed" && params["threadId"]!=root);

  agent.handle_event(event).await.expect("Agent event");

  if let Some((id,params))=request {
   prompts+=1;

   let event_id=agent.pending_requests[&id];
   let stored=agent.store.get_agent_inbox_event(event_id).await.expect("child inbox event");

   assert_eq!(stored.work_item_id,"agent");

   let payload:Value=serde_json::from_str(&stored.payload).expect("stored request");

   assert_eq!(payload["params"],params);
   assert_eq!(payload["ownerThreadId"],root);

   agent.respond_permission(id,serde_json::json!({"action":"accept","content":{"answer":"continue"}})).await.expect("exact child reply");

   assert!(agent.respond_pending_event(event_id,serde_json::json!({"action":"decline"})).await.is_err(),"reply cannot be repeated");
  }

  if done {break;}
 }

 assert_eq!(prompts,usize::from(interactive),"exactly one interactive child prompt");
 assert!(agent.pending_requests.is_empty(),"all child prompts resolved");

 let result:Value=serde_json::from_slice(&fs::read(&record).expect("native MCP result")).expect("result JSON");

 assert_eq!(result["result"],serde_json::json!({"action":"accept","content":if interactive {serde_json::json!({"answer":"continue"})} else {serde_json::json!({})}}),"marker={marker}, native reply={result}");

 let mut child_calls=0;
 let mut continuations=0;

 while let Ok(body)=requests_rx.try_recv() {
  if native_subagent_live::is_child(&body) {
   let output=body["input"].as_array().expect("child input").iter().find(|i|i["type"]=="function_call_output" && i["call_id"]=="child-mcp");

   if let Some(output)=output {assert!(output.to_string().contains("accept"));continuations+=1;} else {child_calls+=1;}
  }
 }

 assert_eq!(child_calls,1,"no automatic MCP retry");
 assert_eq!(continuations,1,"exact child receives the accepted response");
 })).catch_unwind().await;

	process.shutdown().await.expect("shutdown");
	backend.abort();
	outcome.expect("child MCP qualification").expect("child MCP deadline");
}
