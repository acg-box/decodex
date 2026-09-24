//! Verify the declared form extension through real native MCP and Chief replies.
use super::{fixture, native_task_references};
use crate::chief::{ChiefConfig, ChiefCoordinator};
use decodex_codex::app_server_client::{AppServerClient, RequestId, ServerEvent};
use futures_util::FutureExt as _;
use serde_json::{Value, json};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use tokio::io::AsyncWriteExt as _;

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY; isolated native MCP form qualification"]
async fn native_openai_form_negotiates_and_round_trips_through_chief() {
	for (approval, sandbox, opaque) in [
		("on-request", "read-only", false),
		("on-request", "read-only", true),
		("never", "danger-full-access", false),
	] {
		let binary = std::env::var("DECODEX_NATIVE_BINARY").unwrap();
		let home = tempfile::tempdir().unwrap();
		let home_path = home.path().canonicalize().unwrap();
		let server_path = home_path.join("form_server.py");
		let record = home_path.join("mcp_record.json");
		std::fs::write(&server_path, include_str!("native_form_server.py")).unwrap();
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let address = listener.local_addr().unwrap();
		let calls = Arc::new(AtomicUsize::new(0));
		let backend = tokio::spawn(serve(listener, Arc::clone(&calls)));
		std::fs::write(home_path.join("config.toml"), format!(
		"model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\napprovals_reviewer = \"user\"\n[model_providers.fixture]\nname = \"Isolated form fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n[mcp_servers.fixture]\ncommand = \"/usr/bin/python3\"\nargs = [{}, {}, {}]\nrequired = true\n",
		json!(server_path),json!(record),json!(opaque.to_string()))).unwrap();
		let (mut chief, _, _store_home) = fixture().await;
		let mut command = tokio::process::Command::new(binary);
		command
			.arg("app-server")
			.current_dir(&home_path)
			.env_clear()
			.env("HOME", &home_path)
			.env("CODEX_HOME", &home_path)
			.env("PATH", "/usr/bin:/bin");
		let (client, mut events, mut process) = AppServerClient::spawn(&mut command).unwrap();
		chief.client = client;
		chief.config = ChiefConfig::new(
			"gpt-5.6-sol".into(),
			"medium".into(),
			home_path.display().to_string(),
		);
		chief.config.sandbox = sandbox.into();
		chief.config.approval_policy = json!(approval);
		let result = std::panic::AssertUnwindSafe(tokio::time::timeout(
		std::time::Duration::from_secs(40),
		async {
			chief.initialize().await.unwrap();
			chief.start_chief("chief", "Call the fixture MCP tool.").await.unwrap();
			let mut saved = None;
			loop {
				let event = events.recv().await.expect("native form events");
				let request = match &event {
					ServerEvent::Request { id, method, params }
						if method == "mcpServer/elicitation/request" =>
					{
						assert_eq!(params["mode"], "openaiForm");
						assert_eq!(params["_meta"]["fixture/source"], "native-mcp");
						if opaque { assert_eq!(params["requestedSchema"], true); } else {
                            assert_eq!(params["requestedSchema"]["properties"]["answer"]["oneOf"][0]["const"], "wire-value");
                        }
						Some(id.clone())
					},
					_ => None,
				};
				let done = matches!(&event,ServerEvent::Notification {method,..} if method == "turn/completed");
				if let ServerEvent::Notification { method, params } = &event
					&& method == "turn/completed"
				{
					assert_eq!(params["turn"]["status"], "completed", "fixture terminal: {params}");
				}
				chief.handle_event(event).await.unwrap();
				if let Some(id) = request {
					saved = Some(answer_form(&mut chief, &id, opaque).await);
				}
				if done {
					break;
				}
			}
            if approval == "on-request" {
                let event = chief.store.get_chief_inbox_event(saved.expect("native form forwarded")).await.unwrap();
                assert!(event.disposition.is_some());
            } else {
                assert!(saved.is_none(), "native never policy must remain authoritative");
            }
            assert_record(&record, if approval == "never" {"decline"} else if opaque {"cancel"} else {"accept"});
			assert_eq!(calls.load(Ordering::Acquire), 2);
		},
	))
	.catch_unwind()
	.await;
		process.shutdown().await.unwrap();
		backend.abort();
		result
			.expect("native form qualification panicked")
			.expect("native form qualification timed out");
	}
}

async fn serve(listener: tokio::net::TcpListener, calls: Arc<AtomicUsize>) {
	while let Ok((mut socket, _)) = listener.accept().await {
		let body = native_task_references::read_http_body(&mut socket).await;
		let serial = calls.fetch_add(1, Ordering::AcqRel);
		let item = if serial == 0 {
			json!({"type":"function_call","id":"form-call","call_id":"form-call","namespace":"mcp__fixture","name":"form_fixture","arguments":"{}"})
		} else {
			assert!(body.to_string().contains("Fixture form action:"));
			json!({"type":"message","role":"assistant","id":"form-answer","content":[{"type":"output_text","text":"Form complete"}]})
		};
		let frames = [
			json!({"type":"response.created","response":{"id":format!("form-{serial}")}}),
			json!({"type":"response.output_item.done","item":item}),
			json!({"type":"response.completed","response":{"id":format!("form-{serial}")}}),
		];
		let data = frames
			.iter()
			.map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().expect("event type")))
			.collect::<String>();
		let response = format!(
			"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}",
			data.len()
		);
		socket.write_all(response.as_bytes()).await.expect("fixture response");
	}
}

fn assert_record(path: &std::path::Path, action: &str) {
	let recorded: Value =
		serde_json::from_slice(&std::fs::read(path).expect("fixture record")).expect("record JSON");
	assert_eq!(recorded["capabilities"]["extensions"], json!({"openai/elicitation":{"form":{}}}));
	let replies = recorded["replies"].as_array().expect("MCP replies");
	assert_eq!(replies.len(), 1);
	assert_eq!(replies[0]["result"]["action"], action);
	assert_eq!(
		replies[0]["result"]["content"],
		if action == "accept" { json!({"answer":"wire-value"}) } else { Value::Null }
	);
}

async fn answer_form(chief: &mut ChiefCoordinator, id: &RequestId, opaque: bool) -> i64 {
	let event_id = chief.pending_requests[id];
	assert!(
		chief
			.respond_pending_event(
				event_id,
				json!({"action":"accept","content":{"answer":"Display label"}})
			)
			.await
			.is_err()
	);
	if opaque {
		assert!(
			chief
				.respond_pending_event(event_id, json!({"action":"accept","content":null}))
				.await
				.is_err()
		);
	}
	let response = if opaque {
		json!({"action":"cancel","content":null})
	} else {
		json!({"action":"accept","content":{"answer":"wire-value"},"_meta":null})
	};
	chief.respond_pending_event(event_id, response).await.expect("explicit native form reply");
	assert!(
		chief
			.respond_pending_event(event_id, json!({"action":"cancel","content":null}))
			.await
			.is_err()
	);
	event_id
}
