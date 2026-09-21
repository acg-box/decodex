//! Qualify permission-preserving Chief hydration against the installed native server.
use super::*;
use futures_util::FutureExt as _;
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY; isolated native permission restoration"]
async fn native_chief_resume_preserves_selected_profile_policy_and_cwd() {
	let binary = std::env::var("DECODEX_NATIVE_BINARY").unwrap();
	let native_home = tempfile::tempdir().unwrap();
	let home = native_home.path().canonicalize().unwrap();
	let workspace = home.join("selected-workspace");
	std::fs::create_dir_all(workspace.join("writable/private")).unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(listener, Arc::clone(&calls)));
	std::fs::write(home.join("config.toml"), format!(
		"model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\napprovals_reviewer = \"user\"\n[model_providers.fixture]\nname = \"Isolated permissions fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n[permissions.scoped.filesystem]\n\":root\" = \"read\"\n{} = \"write\"\n{} = \"deny\"\n",
		json!(workspace.join("writable")), json!(workspace.join("writable/private"))
	)).unwrap();
	let (mut chief, _, store_home) = fixture().await;
	chief.config =
		ChiefConfig::new("gpt-5.6-sol".into(), "medium".into(), home.display().to_string());
	chief.config.sandbox = "danger-full-access".into();
	chief.config.approval_policy = json!("never");
	for cold in [false, true] {
		if cold {
			let root = decodex_core::DecodexRoot::new(
				store_home.path().canonicalize().unwrap().join("root"),
			)
			.unwrap();
			chief.store = SqliteStore::open(&root.paths()).unwrap();
		}
		let mut command = tokio::process::Command::new(&binary);
		command
			.arg("app-server")
			.current_dir(&home)
			.env_clear()
			.env("HOME", &home)
			.env("CODEX_HOME", &home)
			.env("PATH", "/usr/bin:/bin");
		let (client, mut events, mut process) = AppServerClient::spawn(&mut command).unwrap();
		chief = ChiefCoordinator::new(chief.store.clone(), client, chief.config.clone()).unwrap();
		let result = std::panic::AssertUnwindSafe(tokio::time::timeout(
			std::time::Duration::from_secs(40),
			async {
				chief.initialize().await.unwrap();
				if !cold {
					chief.start_chief("chief", "Reply Done.").await.unwrap();
					finish(&mut chief, &mut events).await;
					let item = chief.store.get_chief_work_item("chief".into()).await.unwrap();
					chief
						.client
						.request(
							"thread/settings/update",
							json!({
								"threadId": item.codex_thread_id, "permissions":"scoped",
								"approvalPolicy":"on-request", "cwd":workspace
							}),
						)
						.await
						.unwrap();
					chief.loaded_threads.clear();
				}
				chief.continue_worker("chief", "Reply Done again.").await.unwrap();
				finish(&mut chief, &mut events).await;
				let item = chief.store.get_chief_work_item("chief".into()).await.unwrap();
				let actual = chief
					.client
					.thread_resume(json!({
						"threadId":item.codex_thread_id, "excludeTurns":true
					}))
					.await
					.unwrap();
				assert_eq!(
					actual["activePermissionProfile"]["id"], "scoped",
					"cold={cold}: {actual}"
				);
				assert_eq!(actual["approvalPolicy"], "on-request", "cold={cold}");
				assert_eq!(actual["cwd"], json!(workspace), "cold={cold}");
				assert_eq!(calls.load(Ordering::Acquire), if cold { 3 } else { 2 });
			},
		))
		.catch_unwind()
		.await;
		process.shutdown().await.unwrap();
		if result.as_ref().is_err() || result.as_ref().is_ok_and(|result| result.is_err()) {
			backend.abort();
		}
		result
			.expect("native permission qualification panicked")
			.expect("native permission qualification timed out");
	}
	backend.abort();
}

async fn finish(
	chief: &mut ChiefCoordinator,
	events: &mut tokio::sync::mpsc::Receiver<ServerEvent>,
) {
	loop {
		let event = events.recv().await.expect("native events");
		let done = if let ServerEvent::Notification { method, params } = &event {
			if method == "turn/completed" {
				assert_eq!(params["turn"]["status"], "completed", "{params}");
				true
			} else {
				false
			}
		} else {
			false
		};
		chief.handle_event(event).await.unwrap();
		if done {
			break;
		}
	}
}

pub(super) async fn serve(listener: tokio::net::TcpListener, calls: Arc<AtomicUsize>) {
	while let Ok((mut socket, _)) = listener.accept().await {
		let _body = native_task_references::read_http_body(&mut socket).await;
		let serial = calls.fetch_add(1, Ordering::AcqRel);
		let id = format!("permissions-{serial}");
		let frames = [
			json!({"type":"response.created","response":{"id":id}}),
			json!({"type":"response.output_item.done","item":{"type":"message","role":"assistant","id":format!("message-{serial}"),"content":[{"type":"output_text","text":"Done."}]}}),
			json!({"type":"response.completed","response":{"id":id,"usage":{"input_tokens":0,"output_tokens":5,"total_tokens":5}}}),
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
