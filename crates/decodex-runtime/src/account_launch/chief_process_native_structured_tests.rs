//! Temporary recap transport qualification against the installed native server.
use super::*;
use decodex_codex::app_server_client::TemporaryStructuredOptions;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated temporary structured request"]
async fn installed_temporary_requests_disable_tools_and_preserve_custom_permissions() {
	tokio::time::timeout(Duration::from_secs(60), qualify()).await.expect("bounded native fixture");
}

async fn qualify() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native temporary fixture");
	let home = tempfile::tempdir().expect("native temporary fixture");
	let listener =
		tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("native temporary fixture");
	let address = listener.local_addr().expect("native temporary fixture");
	let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
	let backend = tokio::spawn(serve_fixture(
		listener,
		calls.clone(),
		None,
		Some(bodies.clone()),
		None,
		|_| json!({"type":"message","id":"recap-output","role":"assistant","content":[{"type":"output_text","text":"{\"summary\":\"Fixture recap\",\"next\":null}"}]}),
	));
	let config = format!(
		"model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\ndefault_permissions=\"recap-restricted\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n[mcp_servers.forbidden]\ncommand=\"must-not-run-recap-tool\"\nrequired=true\n[permissions.recap-restricted.filesystem]\n\":root\"=\"read\"\n\"/private/recap-denied\"=\"deny\"\n"
	);
	std::fs::write(home.path().join("config.toml"), &config).expect("native temporary fixture");
	let mut session = NativeSession::start(&binary, home.path());
	for profile in [None, Some("recap-restricted")] {
		let thread = session
			.client
			.start_temporary_structured(TemporaryStructuredOptions {
				model: "gpt-5.6-sol".into(),
				model_provider: "fixture".into(),
				cwd: home.path().display().to_string(),
				active_permission_profile: profile.map(str::to_owned),
				mcp_server_names: vec!["forbidden".into()],
			})
			.await
			.unwrap_or_else(|error| match error {
				ClientError::Remote(error) => panic!("fixture native error: {}", error.message),
				_ => panic!("fixture: {error}"),
			});
		let id = thread.id().to_owned();
		let (_cancel, watch) = tokio::sync::watch::channel(false);
		let (_placeholder, empty) = tokio::sync::mpsc::channel(1);
		let mut events = std::mem::replace(&mut session.events, empty);
		let (send, receive) = tokio::sync::mpsc::channel(64);
		let (stop, mut stopped) = tokio::sync::oneshot::channel::<()>();
		let forward = tokio::spawn(async move {
			loop {
				tokio::select! {
					_ = &mut stopped => break,
					event = events.recv() => match event {
						Some(event) => if send.send(event).await.is_err() {break;},
						None => break,
					}
				}
			}
			events
		});
		let schema = json!({"type":"object","properties":{"summary":{"type":"string"},"next":{"type":["string","null"]}},"required":["summary","next"],"additionalProperties":false});
		let value = thread
			.run("Summarize the fixture without tools.".into(), schema, None, receive, watch)
			.await
			.expect("native structured result");
		assert_eq!(
			serde_json::from_str::<Value>(&value).expect("native temporary fixture")["summary"],
			"Fixture recap"
		);
		// Unsubscribe detaches this connection; it is not an immediate thread shutdown.
		let _ = stop.send(());
		session.events = tokio::time::timeout(Duration::from_secs(5), forward)
			.await
			.expect("event route stopped")
			.expect("native temporary fixture");
		let listed = session
			.client
			.request("thread/list", json!({"limit":100}))
			.await
			.expect("native temporary fixture");
		assert!(
			!listed["data"]
				.as_array()
				.expect("native temporary fixture")
				.iter()
				.any(|t| t["id"] == id)
		);
	}
	assert_eq!(calls.load(Ordering::Acquire), 2);
	for body in bodies.lock().expect("native temporary fixture").iter() {
		assert!(
			body["tools"].as_array().is_none_or(Vec::is_empty),
			"native temporary request exposed tools"
		);
		assert_eq!(body["text"]["format"]["type"], "json_schema");
	}
	assert_eq!(
		std::fs::read_to_string(home.path().join("config.toml")).expect("native temporary fixture"),
		config
	);
	backend.abort();
}
