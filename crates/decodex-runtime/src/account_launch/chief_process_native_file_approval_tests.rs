//! Qualify pending patch evidence through the installed native process and coordinator.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native file approval"]
async fn installed_native_file_approval_saves_live_diff_before_history_and_declines_once() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().unwrap();
	let target = home.path().join("approval-target.txt");
	let patch = format!(
		"*** Begin Patch\n*** Add File: {}\n+{} REQUIRED FILE SUFFIX\n*** End Patch\n",
		target.display(),
		"界".repeat(30000)
	);
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve_fixture(
		listener,
		calls.clone(),
		None,
		None,
		Some(json!({"input_tokens":0,"output_tokens":0,"total_tokens":0})),
		move |serial| {
			if serial == 0 {
				json!({"type":"custom_tool_call","name":"apply_patch","call_id":"native-patch","input":patch})
			} else {
				json!({"type":"message","id":"done","role":"assistant","content":[{"type":"output_text","text":"Done"}]})
			}
		},
	));
	std::fs::write(home.path().join("config.toml"), format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nstep_model_switching=false\nenable_request_compression=false\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).unwrap();
	let mut session = NativeSession::start(&binary, home.path());
	let root = decodex_core::DecodexRoot::new(home.path().canonicalize().unwrap().join("decodex"))
		.unwrap();
	root.paths().ensure_layout().unwrap();
	let store = decodex_database::SqliteStore::open(&root.paths()).unwrap();
	let mut config = crate::ChiefConfig::new(
		"gpt-5.6-sol".into(),
		"medium".into(),
		home.path().display().to_string(),
	);
	config.sandbox = "read-only".into();
	let mut chief =
		crate::ChiefCoordinator::new(store.clone(), session.client.clone(), config).unwrap();
	tokio::time::timeout(Duration::from_secs(60), async {
		chief.start_chief("chief", "Propose the synthetic patch for user review.").await.unwrap();
		let mut approved_event = None;
		loop {
			let event = session.events.recv().await.expect("native event");
			let request = match &event {
				ServerEvent::Request { method, params, .. } if method == "item/fileChange/requestApproval" => Some(params.clone()),
				_ => None,
			};
			let completed = matches!(&event, ServerEvent::Notification { method, .. } if method == "turn/completed");
			chief.handle_event(event).await.unwrap();
			if let Some(params) = request {
				let event = store.list_pending_chief_events(100).await.unwrap().into_iter().find(|e| e.event_kind == "permission_pending").unwrap();
				assert!(event.payload.len() < 4096);
				let reopened = decodex_database::SqliteStore::open(&root.paths()).unwrap();
				let saved = reopened.get_chief_inbox_event(event.id).await.unwrap();
				let payload: Value = serde_json::from_str(&saved.payload).unwrap();
				assert_eq!(payload["params"], params);
				let diff = crate::chief_detail::saved_file_changes(&payload).expect("live patch retained");
				assert!(diff.len() > 90000 && diff.contains("REQUIRED FILE SUFFIX"));
				let history = session.client.thread_read(json!({"threadId":params["threadId"],"includeTurns":true})).await.unwrap();
				assert!(!history.to_string().contains("REQUIRED FILE SUFFIX"), "qualify live evidence rather than history fallback");
				chief.respond_pending_event(event.id, json!({"decision":"decline"})).await.unwrap();
				assert!(chief.respond_pending_event(event.id, json!({"decision":"accept"})).await.is_err());
				approved_event = Some(event.id);
			}
			if completed { break; }
		}
		assert!(approved_event.is_some());
		assert!(!target.exists());
		assert_eq!(calls.load(Ordering::SeqCst), 2);
	}).await.expect("native approval deadline");
	backend.abort();
}
