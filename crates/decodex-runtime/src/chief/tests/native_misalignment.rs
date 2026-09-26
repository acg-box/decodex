//! Qualify explicit continuation with an installed native process and synthetic provider.
use super::{fixture, native_task_references};
use crate::chief::{ChiefConfig, ChiefCoordinator};
use decodex_codex::app_server_client::{AppServerClient, ServerEvent};
use futures_util::FutureExt as _;
use serde_json::{Value, json};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use tokio::io::AsyncWriteExt as _;

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY; isolated native misalignment qualification"]
async fn native_misalignment_requires_live_confirmation_and_submits_only_once() {
	let home = tempfile::tempdir().unwrap();
	let path = home.path().canonicalize().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(listener, Arc::clone(&calls)));
	std::fs::write(path.join("config.toml"), format!(
		"model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[model_providers.fixture]\nname = \"Isolated continuation fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n"
	)).unwrap();
	let mut command = tokio::process::Command::new(std::env::var("DECODEX_NATIVE_BINARY").unwrap());
	command
		.arg("app-server")
		.current_dir(&path)
		.env_clear()
		.env("HOME", &path)
		.env("CODEX_HOME", &path)
		.env("PATH", "/usr/bin:/bin");
	let (client, mut events, mut process) = AppServerClient::spawn(&mut command).unwrap();
	let (mut chief, _, _database) = fixture().await;
	chief.client = client;
	chief.config =
		ChiefConfig::new("gpt-5.6-sol".into(), "medium".into(), path.display().to_string());
	chief.config.sandbox = "read-only".into();
	let result = std::panic::AssertUnwindSafe(tokio::time::timeout(
		std::time::Duration::from_secs(40),
		async {
			chief.initialize().await.unwrap();
			chief.start_chief("chief", "Inspect the synthetic fixture.").await.unwrap();
			finish_turn(&mut chief, &mut events, "failed").await;
			let review = chief.store.chief_misalignment("chief".into()).await.unwrap().unwrap();
			assert!(review.details_json.as_deref().unwrap().contains("Review the fixture scope."));
			let (_, guard) =
				chief.client.live_misalignment_review(&review.thread_id, &review.turn_id).unwrap();
			let token = crate::chief::misalignment::review_token(&review, &guard).unwrap();
			assert_eq!(calls.load(Ordering::Acquire), 1);
			assert!(
				chief
					.continue_misalignment("chief", review.clone(), "stale", "old-review")
					.await
					.is_err()
			);
			assert_eq!(calls.load(Ordering::Acquire), 1);
			chief
				.continue_misalignment("chief", review.clone(), "explicit-confirmation", &token)
				.await
				.unwrap();
			finish_turn(&mut chief, &mut events, "completed").await;
			assert!(chief.store.chief_misalignment("chief".into()).await.unwrap().is_none());
			assert!(chief.continue_misalignment("chief", review, "repeat", &token).await.is_err());
			assert_eq!(calls.load(Ordering::Acquire), 2);
		},
	))
	.catch_unwind()
	.await;
	process.shutdown().await.unwrap();
	backend.abort();
	result.expect("native continuation panicked").expect("native continuation timed out");
}

async fn finish_turn(
	chief: &mut ChiefCoordinator,
	events: &mut tokio::sync::mpsc::Receiver<ServerEvent>,
	status: &str,
) {
	loop {
		let event = events.recv().await.expect("native event");
		let done = matches!(&event, ServerEvent::Notification { method, .. } if method == "turn/completed");
		if let ServerEvent::Notification { method, params } = &event
			&& method == "turn/completed"
		{
			assert_eq!(params["turn"]["status"], status, "native terminal: {params}");
		}
		chief.handle_event(event).await.unwrap();
		if done {
			return;
		}
	}
}

async fn serve(listener: tokio::net::TcpListener, calls: Arc<AtomicUsize>) {
	while let Ok((mut socket, _)) = listener.accept().await {
		let body = native_task_references::read_http_body(&mut socket).await;
		let serial = calls.fetch_add(1, Ordering::AcqRel);
		let frames: Vec<Value> = if serial == 0 {
			vec![
				json!({"type":"response.created","response":{"id":"first"}}),
				json!({"type":"response.failed","response":{"id":"first","status":"failed","error":{
					"code":"misalignment_policy_violation","message":"Synthetic fixture requires review.",
					"misalignment":{"error_type":"unsafe_activity","detailed_explanation":"Review the fixture scope.","steer":{"message":"Continue within the fixture scope."}}
				}}}),
			]
		} else {
			assert_eq!(serial, 1, "no automatic retry or repeated continuation");
			assert!(body.to_string().contains("Continue within the fixture scope."));
			vec![
				json!({"type":"response.created","response":{"id":"continued"}}),
				json!({"type":"response.output_item.done","item":{"type":"message","role":"assistant","id":"answer","content":[{"type":"output_text","text":"Completed within scope."}]}}),
				json!({"type":"response.completed","response":{"id":"continued"}}),
			]
		};
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
