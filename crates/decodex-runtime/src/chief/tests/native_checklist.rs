//! Qualify opt-in native checklist notifications and their history boundary.
use super::*;
use futures_util::FutureExt as _;
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY; isolated native checklist history"]
async fn native_checklist_notifications_are_not_replayed_by_history() {
	let home = tempfile::tempdir().unwrap();
	let path = home.path().canonicalize().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(listener, Arc::clone(&calls)));
	std::fs::write(path.join("config.toml"),format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\ntools.update_plan.enabled = true\n[model_providers.fixture]\nname = \"Isolated checklist fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();
	let mut thread = String::new();
	for cold in [false, true] {
		let (mut chief, _sent, _store_home) = fixture().await;
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
		let result = std::panic::AssertUnwindSafe(tokio::time::timeout(std::time::Duration::from_secs(20), async {
            chief.initialize().await.unwrap();
            if !cold {
                let response = chief.client.thread_start(json!({"model":"gpt-5.6-sol","cwd":path,"approvalPolicy":"never","sandbox":"read-only"})).await.unwrap();
                thread = response["thread"]["id"].as_str().unwrap().into();
                ChiefCoordinator::reserve_root(&chief.store,"chief","Checklist fixture").await.unwrap();
                chief.store.bind_chief_thread("chief".into(),thread.clone()).await.unwrap();
                chief.store.begin_chief_dispatch("chief".into()).await.unwrap();
                let started = chief.client.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Track the fixture checklist.","text_elements":[]}]})).await.unwrap();
                chief.store.acknowledge_chief_dispatch("chief".into(),started["turn"]["id"].as_str().unwrap().into()).await.unwrap();
                let mut observed = Vec::new();
                loop {
                    if let ServerEvent::Notification {method,params} = events.recv().await.unwrap() {
                        match method.as_str() {
                            "turn/plan/updated" => {
                                assert_eq!(params["threadId"],thread);
                                assert_eq!(params["turnId"],started["turn"]["id"]);
                                chief.handle_event(ServerEvent::Notification {method,params:params.clone()}).await.unwrap();
                                observed.push(params);
                            },
                            "turn/completed" => { assert_eq!(params["turn"]["status"],"completed"); break; },
                            _ => {},
                        }
                    }
                }
                assert_eq!(observed.len(),2);
                assert_eq!(observed[0]["plan"][0]["status"],"inProgress");
                assert_eq!(observed[1]["plan"][0]["status"],"completed");
                assert_eq!(observed[1]["plan"][1]["status"],"pending");
                assert_eq!(observed[1]["explanation"],"First step verified");
                let (receipts, _) = chief.store.read_chief_transcript("chief".into(),None,32).await.unwrap();
                assert_eq!(receipts.len(),1);
                assert_eq!(receipts[0].event_kind,"plan_updated");
                assert!(receipts[0].payload.contains("Completed"));
                assert!(receipts[0].payload.contains("First step verified"));
                assert!(chief.store.list_chief_wake_events("chief".into(),32).await.unwrap().is_empty());
            }
            let mut cursor = None;
            let mut texts = Vec::new();
            loop {
                let page = chief.client.thread_timeline_page(&thread,cursor.as_deref(),1).await.unwrap();
                let projected = super::super::timeline::project(&thread,&page).unwrap();
                texts.push(serde_json::to_string(&page).unwrap());
                cursor = projected.next_cursor;
                if cursor.is_none() { break; }
            }
            let history = texts.join("\n");
            assert!(history.contains("Checklist fixture done"));
            assert!(!history.contains("PRIVATE_CHECKLIST_STEP"),"checklist steps unexpectedly gained native history support: {history}");
            while let Ok(event) = events.try_recv() {
                assert!(!matches!(event,ServerEvent::Notification {method,..} if method == "turn/plan/updated"));
            }
        })).catch_unwind().await;
		process.shutdown().await.unwrap();
		if !matches!(result, Ok(Ok(()))) {
			backend.abort();
		}
		match result {
			Ok(result) => result.unwrap(),
			Err(panic) => std::panic::resume_unwind(panic),
		}
	}
	assert_eq!(calls.load(Ordering::Acquire), 3);
	backend.abort();
}

async fn serve(listener: tokio::net::TcpListener, calls: Arc<AtomicUsize>) {
	while let Ok((mut socket, _)) = listener.accept().await {
		let _request = native_task_references::read_http_body(&mut socket).await;
		let serial = calls.fetch_add(1, Ordering::AcqRel);
		let item = if serial < 2 {
			json!({"type":"function_call","id":format!("item-{serial}"),"call_id":format!("call-{serial}"),"name":"update_plan","arguments":json!({"explanation":if serial == 0 { "Starting checklist" } else { "First step verified" },"plan":[{"step":"PRIVATE_CHECKLIST_STEP one","status":if serial == 0 { "in_progress" } else { "completed" }},{"step":"PRIVATE_CHECKLIST_STEP two","status":"pending"}]}).to_string()})
		} else {
			json!({"type":"message","role":"assistant","id":"message","phase":"final_answer","content":[{"type":"output_text","text":"Checklist fixture done"}]})
		};
		let frames = [
			json!({"type":"response.created","response":{"id":format!("response-{serial}")}}),
			json!({"type":"response.output_item.added","output_index":0,"item":item}),
			json!({"type":"response.output_item.done","output_index":0,"item":item}),
			json!({"type":"response.completed","response":{"id":format!("response-{serial}"),"usage":{"input_tokens":1,"output_tokens":5,"total_tokens":6}}}),
		];
		let body = frames
			.iter()
			.map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap()))
			.collect::<String>();
		socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
	}
}
