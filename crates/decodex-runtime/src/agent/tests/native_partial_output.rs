//! Interrupted streams remain display-only when Codex omits their unfinished items.
use super::*;
use futures_util::FutureExt as _;
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};

const PARTIAL: &str = "Intro.\n\n$$\n\\frac{a+b+c+d+e+f}{g+h}";

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY; isolated interrupted answer and plan"]
async fn native_interrupted_output_is_retained_without_replay() {
	for plan in [false, true] {
		interrupted(plan).await;
	}
}

async fn interrupted(plan: bool) {
	let home = tempfile::tempdir().unwrap();
	let path = home.path().canonicalize().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(listener, Arc::clone(&calls), plan));

	std::fs::write(path.join("config.toml"),format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[features]\ncollaboration_modes = true\n[model_providers.fixture]\nname = \"Interrupted output fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();

	let (mut agent, _, store_home) = fixture().await;
	let mut command = tokio::process::Command::new(std::env::var("DECODEX_NATIVE_BINARY").unwrap());

	command
		.arg("app-server")
		.current_dir(&path)
		.env_clear()
		.env("HOME", &path)
		.env("CODEX_HOME", &path)
		.env("PATH", "/usr/bin:/bin");

	let (client, mut events, mut process) = AppServerClient::spawn(&mut command).unwrap();

	agent.client = client;

	let result = std::panic::AssertUnwindSafe(tokio::time::timeout(std::time::Duration::from_secs(30), async {
        agent.initialize().await.unwrap();

        let response = agent.client.thread_start(json!({"model":"gpt-5.6-sol","cwd":path,"approvalPolicy":"never","sandbox":"read-only"})).await.unwrap();
        let thread = response["thread"]["id"].as_str().unwrap();

        AgentCoordinator::reserve_root(&agent.store,"agent","Partial output").await.unwrap();

        agent.store.bind_agent_thread("agent".into(),thread.into()).await.unwrap();
        agent.store.begin_agent_dispatch("agent".into()).await.unwrap();

        let mut params = json!({"threadId":thread,"input":[{"type":"text","text":"Write the fixture response.","text_elements":[]}]});

        if plan { params["collaborationMode"] = json!({"mode":"plan","settings":{"model":"gpt-5.6-sol","reasoning_effort":"medium","developer_instructions":null}}); }

        let started = agent.client.turn_start(params).await.unwrap();
        let turn = started["turn"]["id"].as_str().unwrap();

        agent.store.acknowledge_agent_dispatch("agent".into(),turn.into()).await.unwrap();

        let method_expected = if plan { "item/plan/delta" } else { "item/agentMessage/delta" };
        let mut interrupted = false;

        loop {
            let event = events.recv().await.unwrap();
            let terminal = matches!(&event,ServerEvent::Notification {method,..} if method=="turn/completed");
            let delta = matches!(&event,ServerEvent::Notification {method,..} if method==method_expected);

            agent.handle_event(event).await.unwrap();

            if delta && !interrupted {
                let live = agent.store.read_agent_output("agent".into()).await.unwrap();

                if live.iter().any(|row|row.text==PARTIAL) {
                    agent.client.turn_interrupt(json!({"threadId":thread,"turnId":turn})).await.unwrap();

                    interrupted = true;
                }
            }

            if terminal { break; }
        }

        assert!(interrupted);

        let history = agent.client.thread_read_turn_items(thread,turn).await.unwrap();

        assert!(!history.as_array().unwrap().iter().any(|item| matches!(item["type"].as_str(),Some("agentMessage" | "plan"))));

        let root = decodex_core::DecodexRoot::new(store_home.path().canonicalize().unwrap().join("root")).unwrap();
        let reopened = SqliteStore::open(&root.paths()).unwrap();
        let (events, live) = reopened.read_agent_transcript("agent".into(),None,32).await.unwrap();

        assert!(live.is_empty());

        let rendered = crate::application::render_agent_history_for_test(events);
        let partial: Vec<_> = rendered.iter().filter(|entry| entry.kind==if plan {"partial_plan"} else {"partial_answer"}).collect();

        assert_eq!(partial.len(),1);
        assert_eq!(partial[0].text,PARTIAL);
        assert_eq!(calls.load(Ordering::Acquire),1,"display readback never requests inference");
    })).catch_unwind().await;

	process.shutdown().await.unwrap();
	backend.abort();
	result.expect("native partial fixture panicked").expect("native partial fixture timed out");
}

async fn serve(listener: tokio::net::TcpListener, calls: Arc<AtomicUsize>, plan: bool) {
	while let Ok((mut socket, _)) = listener.accept().await {
		let _ = native_task_references::read_http_body(&mut socket).await;

		calls.fetch_add(1, Ordering::AcqRel);

		let text = if plan { format!("<proposed_plan>\n{PARTIAL}") } else { PARTIAL.into() };
		let frames = [
			json!({"type":"response.created","response":{"id":"partial-response"}}),
			json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","role":"assistant","id":"partial-item","phase":"final_answer","content":[]}}),
			json!({"type":"response.output_text.delta","item_id":"partial-item","output_index":0,"content_index":0,"delta":text}),
		];
		let body = frames
			.iter()
			.map(|value| format!("event: {}\ndata: {value}\n\n", value["type"].as_str().unwrap()))
			.collect::<String>();

		socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\n{body}").as_bytes()).await.unwrap();
		socket.flush().await.unwrap();
		// Keep the provider stream unfinished until the test explicitly interrupts it.
		std::future::pending::<()>().await;
	}
}
