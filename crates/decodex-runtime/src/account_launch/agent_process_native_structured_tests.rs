//! Temporary recap transport qualification against the installed native server.
use std::{
	env, fs, mem,
	sync::{Mutex, atomic::AtomicUsize},
};

use tokio::{
	net::TcpListener,
	sync::{mpsc, oneshot, watch},
	time,
};

use crate::account_launch::agent_process::native_tests::{
	self, Arc, ClientError, Duration, NativeSession, Ordering, Value,
};
use decodex_codex::app_server_client::TemporaryStructuredOptions;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated temporary structured request"]
async fn installed_temporary_requests_isolate_environment_and_preserve_custom_permissions() {
	time::timeout(Duration::from_secs(60), qualify()).await.expect("bounded native fixture");
}

async fn qualify() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native temporary fixture");
	let home = tempfile::tempdir().expect("native temporary fixture");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("native temporary fixture");
	let address = listener.local_addr().expect("native temporary fixture");
	let calls = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		calls.clone(),
		None,
		Some(bodies.clone()),
		None,
		|_| serde_json::json!({"type":"message","id":"recap-output","role":"assistant","content":[{"type":"output_text","text":"{\"summary\":\"Fixture recap\",\"next\":null}"}]}),
	));
	let config = format!(
		"model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\ndefault_permissions=\"recap-restricted\"\napproval_policy=\"on-request\"\n[features]\nstable_environment_tools=true\nenable_request_compression=false\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n[mcp_servers.forbidden]\ncommand=\"must-not-run-recap-tool\"\nrequired=true\n[permissions.recap-restricted.filesystem]\n\":root\"=\"read\"\n\"/private/recap-denied\"=\"deny\"\n"
	);

	fs::write(home.path().join("config.toml"), &config).expect("native temporary fixture");

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
		let (permissions, _) =
			session.client.configured_task_permissions(&id).expect("native temporary permissions");
		assert_eq!(permissions.approval_policy, serde_json::json!("on-request"));

		let (_cancel, watch) = watch::channel(false);
		let (_placeholder, empty) = mpsc::channel(1);
		let mut events = mem::replace(&mut session.events, empty);
		let (send, receive) = mpsc::channel(64);
		let (stop, mut stopped) = oneshot::channel::<()>();
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
		let schema = serde_json::json!({"type":"object","properties":{"summary":{"type":"string"},"next":{"type":["string","null"]}},"required":["summary","next"],"additionalProperties":false});
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

		session.events = time::timeout(Duration::from_secs(5), forward)
			.await
			.expect("event route stopped")
			.expect("native temporary fixture");

		let listed = session
			.client
			.request("thread/list", serde_json::json!({"limit":100}))
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
		assert_isolated_tool_catalog(body);
		assert_eq!(body["text"]["format"]["type"], "json_schema");
	}

	assert_eq!(
		fs::read_to_string(home.path().join("config.toml")).expect("native temporary fixture"),
		config
	);

	backend.abort();
}

// Responses Lite carries the model catalog in input, not the top-level tools field.
// The bundled model forces Code Mode even when the feature overrides are false.
fn assert_isolated_tool_catalog(body: &Value) {
	assert!(body["tools"].as_array().is_none_or(Vec::is_empty));

	let catalogs: Vec<_> = body["input"]
		.as_array()
		.expect("Responses Lite input")
		.iter()
		.filter(|item| item["type"] == "additional_tools")
		.collect();
	assert_eq!(catalogs.len(), 1);
	let namespaces = catalogs[0]["tools"].as_array().expect("native tool namespaces");
	assert_eq!(namespaces.len(), 1);
	assert_eq!(namespaces[0]["name"], "functions");
	let tools = namespaces[0]["tools"].as_array().expect("Code Mode tools");
	assert_eq!(
		tools.iter().map(|tool| tool["name"].as_str().expect("tool name")).collect::<Vec<_>>(),
		["exec", "wait"]
	);
	let nested: Vec<_> = tools[0]["description"]
		.as_str()
		.expect("Code Mode catalog")
		.lines()
		.filter_map(|line| line.strip_prefix("### `").and_then(|name| name.strip_suffix('`')))
		.collect();
	// These native resource helpers remain registered with zero configured servers.
	// No environment, app, dynamic, or MCP server tool may enter the catalog.
	assert_eq!(nested, ["list_mcp_resource_templates", "list_mcp_resources", "read_mcp_resource"]);
}
