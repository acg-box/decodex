//! Observe native goal persistence through the retained bridge without model work.
use super::*;
use decodex_codex::app_server_client::NativeThreadGoalStatus;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native goal qualification"]
async fn installed_native_goal_reads_preserve_native_state_after_restart() {
	tokio::time::timeout(Duration::from_secs(45), qualify()).await.expect("bounded fixture");
}

async fn qualify() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().expect("native goal fixture");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("native goal fixture");
	let address = listener.local_addr().expect("native goal fixture");
	let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let backend = tokio::spawn(serve(listener, Arc::clone(&requests)));
	std::fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\ncli_auth_credentials_store=\"file\"\n[features]\ngoals=true\n[model_providers.fixture]\nname=\"Isolated goal fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("native goal fixture");
	let mut child = tokio::process::Command::new(&binary)
		.arg("app-server")
		.env_clear()
		.env("HOME", home.path())
		.env("CODEX_HOME", home.path())
		.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
		.current_dir(home.path())
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::null())
		.kill_on_drop(true)
		.spawn()
		.expect("native goal fixture");
	let (client, mut events) = AppServerClient::from_io(
		child.stdout.take().expect("native goal fixture"),
		child.stdin.take().expect("native goal fixture"),
	);
	client.initialize(json!({"clientInfo":{"name":"decodex_goal_fixture","version":"0.1"},"capabilities":{"experimentalApi":true}})).await.expect("native goal fixture");
	let started = client
		.thread_start(json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only"}))
		.await
		.expect("native goal fixture");
	let thread = started["thread"]["id"].as_str().expect("native goal fixture").to_owned();
	assert_eq!(client.thread_goal(&thread).await.expect("native goal fixture"), None);
	let set=client.request("thread/goal/set",json!({"threadId":thread,"objective":"Observe a paused native goal","status":"paused","tokenBudget":12345})).await.expect("native goal fixture");
	let before =
		client.thread_goal(&thread).await.expect("native goal fixture").expect("native goal");
	assert_eq!(before.status, NativeThreadGoalStatus::Paused);
	assert_eq!(before.token_budget, Some(12345));
	assert_eq!(before.tokens_used, 0);
	assert_eq!(before.time_used_seconds, 0);
	assert_eq!(serde_json::to_value(&before).expect("native goal fixture"), set["goal"]);
	loop {
		let event = events.recv().await.expect("native goal fixture");
		if let ServerEvent::Notification { method, params } = event
			&& method == "thread/goal/updated"
			&& params["threadId"] == thread
		{
			assert_eq!(params["goal"], set["goal"]);
			break;
		}
	}
	// A second native process reads the persisted source through the production bridge.
	let observer = NativeSession::start(&binary, home.path());
	assert_eq!(
		observer.client.thread_goal(&thread).await.expect("native goal fixture"),
		Some(before.clone())
	);
	client
		.request("thread/goal/clear", json!({"threadId":thread}))
		.await
		.expect("native goal fixture");
	assert_eq!(observer.client.thread_goal(&thread).await.expect("native goal fixture"), None);
	client
		.request(
			"thread/goal/set",
			json!({"threadId":thread,"objective":before.objective,"status":"paused","tokenBudget":null}),
		)
		.await
		.expect("native goal fixture");
	let unbudgeted = observer
		.client
		.thread_goal(&thread)
		.await
		.expect("native goal fixture")
		.expect("native goal fixture");
	assert_eq!(unbudgeted.token_budget, None);
	drop(observer);
	child.kill().await.expect("native goal fixture");
	child.wait().await.expect("native goal fixture");
	let reopened = NativeSession::start(&binary, home.path());
	assert_eq!(
		reopened.client.thread_goal(&thread).await.expect("native goal fixture"),
		Some(unbudgeted)
	);
	assert_eq!(requests.load(Ordering::Acquire), 0, "goal observation must not start inference");
	drop(reopened);
	qualify_disabled(&binary, home.path(), &thread).await;
	backend.abort();
}

async fn qualify_disabled(binary: &std::ffi::OsStr, home: &std::path::Path, thread: &str) {
	let path = home.join("config.toml");
	let config = std::fs::read_to_string(&path).expect("fixture config");
	std::fs::write(path, config.replace("goals=true", "goals=false"))
		.expect("disable fixture goals");
	let disabled = NativeSession::start(binary, home);
	assert!(
		matches!(disabled.client.thread_goal(thread).await,Err(ClientError::Remote(error)) if error.code==-32600 && error.message=="goals feature is disabled")
	);
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated active goal accounting"]
async fn installed_active_goal_preserves_native_budget_and_elapsed_accounting() {
	tokio::time::timeout(Duration::from_secs(45), qualify_active())
		.await
		.expect("bounded active fixture");
}
async fn qualify_active() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().expect("fixture home");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("fixture listener");
	let address = listener.local_addr().expect("fixture address");
	let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	std::fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\ncli_auth_credentials_store=\"file\"\n[features]\ngoals=true\n[model_providers.fixture]\nname=\"Isolated active goal fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("fixture config");
	let mut child = tokio::process::Command::new(&binary)
		.arg("app-server")
		.env_clear()
		.env("HOME", home.path())
		.env("CODEX_HOME", home.path())
		.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
		.current_dir(home.path())
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::null())
		.kill_on_drop(true)
		.spawn()
		.expect("native goal fixture");
	let (client, mut events) = AppServerClient::from_io(
		child.stdout.take().expect("native goal fixture"),
		child.stdin.take().expect("native goal fixture"),
	);
	client.initialize(json!({"clientInfo":{"name":"decodex_goal_fixture","version":"0.1"},"capabilities":{"experimentalApi":true}})).await.expect("native goal fixture");

	let started = client
		.thread_start(json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only"}))
		.await
		.expect("start thread");
	let thread = started["thread"]["id"].as_str().expect("thread").to_owned();
	client.request("thread/goal/set",json!({"threadId":thread,"objective":"Return a short fixture answer","status":"active","tokenBudget":1})).await.expect("activate goal");
	// Hold the fake provider until the active goal has accumulated a whole second.
	tokio::time::sleep(Duration::from_millis(1200)).await;
	let backend = tokio::spawn(serve_fixture(
		listener,
		requests.clone(),
		None,
		None,
		Some(
			json!({"input_tokens":20,"input_tokens_details":{"cached_tokens":4},"output_tokens":10,"output_tokens_details":{"reasoning_tokens":2},"total_tokens":30}),
		),
		|serial| json!({"type":"message","role":"assistant","id":format!("goal-answer-{serial}"),"content":[{"type":"output_text","text":"Fixture result"}]}),
	));
	let observed = loop {
		let event = events.recv().await.expect("goal event");
		if let ServerEvent::Notification { method, params } = event
			&& method == "thread/goal/updated"
			&& params["threadId"] == thread
			&& params["goal"]["status"] == "budgetLimited"
		{
			break params["goal"].clone();
		}
	};
	// Budget-limited time must stop advancing, and no new inference may start.
	tokio::time::sleep(Duration::from_millis(1200)).await;
	let goal = client.thread_goal(&thread).await.expect("goal read").expect("goal");
	assert_eq!(goal.status, NativeThreadGoalStatus::BudgetLimited);
	assert_eq!(goal.token_budget, Some(1));
	assert_eq!(
		goal.tokens_used, 26,
		"native goal excludes cached input and does not add reasoning twice"
	);
	assert!(goal.time_used_seconds >= 1);
	assert_eq!(observed["tokensUsed"], 26);
	assert_eq!(observed["timeUsedSeconds"], goal.time_used_seconds);
	child.kill().await.expect("stop fixture");
	child.wait().await.expect("reap fixture");
	let reopened = NativeSession::start(&binary, home.path());
	assert_eq!(reopened.client.thread_goal(&thread).await.expect("restart read"), Some(goal));
	assert_eq!(
		requests.load(Ordering::Acquire),
		1,
		"budget stop and observer must not start another inference"
	);
	drop(reopened);
	backend.abort();
}
