//! Verify native folder trust without granting trust or replaying input.
use super::*;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native folder trust qualification"]
async fn installed_native_config_reads_preserve_folder_trust_without_granting_it() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let root = tempfile::tempdir().unwrap();
	let home = root.path().join("home");

	std::fs::create_dir(&home).unwrap();

	let mut directories = Vec::new();

	for name in ["trusted", "untrusted", "unknown"] {
		let directory = root.path().join(name);

		std::fs::create_dir_all(directory.join(".codex")).unwrap();
		std::fs::create_dir(directory.join(".git")).unwrap();
		std::fs::write(directory.join(".codex/config.toml"), "model_reasoning_effort = \"low\"\n")
			.unwrap();

		directories.push(directory.canonicalize().unwrap());
	}

	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let backend = tokio::spawn(serve(listener, calls.clone()));
	let config = format!(
		"model_reasoning_effort = \"high\"\n[projects.{}]\ntrust_level = \"trusted\"\n[projects.{}]\ntrust_level = \"untrusted\"\n",
		serde_json::to_string(directories[0].to_str().unwrap()).unwrap(),
		serde_json::to_string(directories[1].to_str().unwrap()).unwrap()
	);
	let config = format!(
		"model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n{config}\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n[features]\nenable_request_compression=false\n"
	);

	std::fs::write(home.join("config.toml"), &config).unwrap();

	let mut session = NativeSession::start(&binary, &home.canonicalize().unwrap());

	for index in [0, 1, 2, 0] {
		let result = session
			.client
			.request("config/read", json!({"cwd":directories[index],"includeLayers":true}))
			.await
			.unwrap();

		assert_eq!(
			result["config"]["model_reasoning_effort"],
			if index == 0 { "low" } else { "high" }
		);

		let layers = result["layers"].as_array().unwrap();
		let project = layers
			.iter()
			.find(|layer| layer["name"]["type"] == "project")
			.expect("native project layer");

		assert_eq!(project["disabledReason"].as_str().is_some(), index != 0);
	}

	assert_eq!(std::fs::read_to_string(home.join("config.toml")).unwrap(), config);

	let started = session.client.thread_start(json!({"cwd":directories[0],"model":"gpt-5.6-sol","approvalPolicy":"never","sandbox":"read-only"})).await.unwrap();

	assert_eq!(started["reasoningEffort"], "low");

	let thread = started["thread"]["id"].as_str().unwrap();

	session
		.client
		.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Say done"}]}))
		.await
		.unwrap();

	tokio::time::timeout(Duration::from_secs(20), async {
		loop {
			if let ServerEvent::Notification { method, params } =
				session.events.recv().await.unwrap()
				&& method == "turn/completed"
			{
				assert_eq!(params["turn"]["status"], "completed");

				break;
			}
		}
	})
	.await
	.unwrap();
	// Simulate another client revoking trust after the task is loaded.
	let revoked = config.replacen("trust_level = \"trusted\"", "trust_level = \"untrusted\"", 1);

	std::fs::write(home.join("config.toml"), &revoked).unwrap();

	let current = session
		.client
		.request("config/read", json!({"cwd":directories[0],"includeLayers":true}))
		.await
		.unwrap();

	assert_eq!(current["config"]["model_reasoning_effort"], "high");

	let resumed =
		session.client.thread_resume(json!({"threadId":thread,"excludeTurns":true})).await.unwrap();

	assert_eq!(
		resumed["reasoningEffort"], "low",
		"Warm task retains its loaded settings after folder trust changes"
	);
	assert_eq!(std::fs::read_to_string(home.join("config.toml")).unwrap(), revoked);
	assert_eq!(calls.load(Ordering::Acquire), 1);

	drop(session);

	backend.abort();
}
