//! Native proposed plans retain completed text through paged history and restart.
use super::*;
use futures_util::FutureExt as _;
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
const FINAL_PLAN: &str = "# Final plan\n1. Inspect source\n2. Verify changes\n";

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY; isolated native proposed plan history"]
async fn native_proposed_plan_history_survives_restart_without_model_replay() {
	let home = tempfile::tempdir().unwrap();
	let path = home.path().canonicalize().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(listener, Arc::clone(&calls)));
	std::fs::write(path.join("config.toml"),format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[features]\ncollaboration_modes = true\n[model_providers.fixture]\nname = \"Isolated plan fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();
	let mut thread = String::new();
	for cold in [false, true] {
		let (mut chief, _, _store_home) = fixture().await;
		let mut command =
			tokio::process::Command::new(std::env::var("DECODEX_NATIVE_BINARY").unwrap());
		command
			.arg("app-server")
			.current_dir(&path)
			.env_clear()
			.env("HOME", &path)
			.env("CODEX_HOME", &path)
			.env("PATH", "/usr/bin:/bin");
		let (client, mut events, mut process) = AppServerClient::spawn(&mut command).unwrap();
		chief.client = client;
		let result = std::panic::AssertUnwindSafe(tokio::time::timeout(std::time::Duration::from_secs(20),async {
   chief.initialize().await.unwrap();
   if !cold {
    let response = chief.client.thread_start(json!({"model":"gpt-5.6-sol","cwd":path,"approvalPolicy":"never","sandbox":"read-only"})).await.unwrap();
    thread = response["thread"]["id"].as_str().unwrap().into();
    ChiefCoordinator::reserve_root(&chief.store,"chief","Plan fixture").await.unwrap();
    chief.store.bind_chief_thread("chief".into(),thread.clone()).await.unwrap();
    chief.store.begin_chief_dispatch("chief".into()).await.unwrap();
    let started = chief.client.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Propose the fixture plan.","text_elements":[]}],"collaborationMode":{"mode":"plan","settings":{"model":"gpt-5.6-sol","reasoning_effort":"medium","developer_instructions":null}}})).await.unwrap();
    chief.store.acknowledge_chief_dispatch("chief".into(),started["turn"]["id"].as_str().unwrap().into()).await.unwrap();
    let mut streamed = String::new();
    let mut completed_plan = None;
    loop {
     if let ServerEvent::Notification {method,params} = events.recv().await.unwrap() {
      chief.handle_event(ServerEvent::Notification {method:method.clone(),params:params.clone()}).await.unwrap();
      if method == "item/plan/delta" {
       let output = chief.store.read_chief_output("chief".into()).await.unwrap();
       let plan = output.iter().find(|item|item.kind=="plan").expect("native plan reaches live store");
       assert_eq!(plan.text,"Draft only\n");
      }
      match method.as_str() {
       "item/plan/delta" => streamed.push_str(params["delta"].as_str().unwrap()),
       "item/completed" if params["item"]["type"] == "plan" => completed_plan = Some(params["item"]["text"].as_str().unwrap().to_owned()),
       "turn/completed" => { assert_eq!(params["turn"]["status"],"completed"); break; },
       _ => {},
      }
     }
    }
    assert_eq!(streamed,"Draft only\n");
    assert_eq!(completed_plan.as_deref(),Some(FINAL_PLAN));
    assert!(chief.store.read_chief_transcript("chief".into(),None,32).await.unwrap().0.iter().all(|event|event.event_kind!="partial_output"),"completed native plan must not become unfinished fallback");
   }
   let mut cursor = None;
   let mut plans = Vec::new();
   let mut observed = Vec::new();
   for _ in 0..20 {
    let page = chief.client.thread_timeline_page(&thread,cursor.as_deref(),1).await.unwrap();
    let projected = super::super::timeline::project(&thread,&page).unwrap();
    for entry in projected.entries {
     observed.push(format!("{:?}",entry.content));
     if let decodex_protocol::ChiefTimelineContent::Item {kind,text,..} = entry.content && kind == "plan" { plans.push(text); }
    }
    cursor = projected.next_cursor;
    if cursor.is_none() { break; }
   }
   assert!(cursor.is_none());
   assert_eq!(plans,vec![FINAL_PLAN],"cold={cold}: {observed:?}");
   assert_eq!(calls.load(Ordering::Acquire),1,"reads never request inference");
  })).catch_unwind().await;
		process.shutdown().await.unwrap();
		if !matches!(result, Ok(Ok(()))) {
			backend.abort();
		}
		result.expect("native plan fixture panicked").expect("native plan fixture timed out");
	}
	backend.abort();
}

async fn serve(listener: tokio::net::TcpListener, calls: Arc<AtomicUsize>) {
	while let Ok((mut socket, _)) = listener.accept().await {
		let _ = native_task_references::read_http_body(&mut socket).await;
		calls.fetch_add(1, Ordering::AcqRel);
		let frames = [
			json!({"type":"response.created","response":{"id":"response"}}),
			json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","role":"assistant","id":"message","phase":"final_answer","content":[]}}),
			json!({"type":"response.output_text.delta","item_id":"message","output_index":0,"content_index":0,"delta":"<proposed_plan>\nDraft only\n</proposed_plan>\n"}),
			json!({"type":"response.output_item.done","output_index":0,"item":{"type":"message","role":"assistant","id":"message","phase":"final_answer","content":[{"type":"output_text","text":format!("<proposed_plan>\n{FINAL_PLAN}</proposed_plan>\n")}]}}),
			json!({"type":"response.completed","response":{"id":"response","usage":{"input_tokens":1,"output_tokens":5,"total_tokens":6}}}),
		];
		let data = frames
			.iter()
			.map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap()))
			.collect::<String>();
		socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}",data.len()).as_bytes()).await.unwrap();
	}
}
