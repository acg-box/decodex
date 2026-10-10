//! Prediction forks inherit context without entering durable history or replaying the parent.
use crate::account_launch::agent_process::native_tests::{
	self, Arc, Duration, NativeSession, Ordering, ServerEvent, Value,
};
use std::{
	env, fs, mem,
	sync::{Mutex, atomic::AtomicUsize},
};
use tokio::{
	net::TcpListener,
	sync::{mpsc, oneshot, watch},
	time,
};
#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native prediction fork"]
async fn installed_prediction_fork_inherits_context_and_cleans_up_without_parent_replay() {
	time::timeout(Duration::from_secs(45), qualify()).await.expect("prediction deadline");
}
async fn qualify() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native binary");
	let home = tempfile::tempdir().expect("fixture");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("fixture");
	let address = listener.local_addr().expect("fixture");
	let calls = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		calls.clone(),
		None,
		Some(bodies.clone()),
		None,
		|_| serde_json::json!({"type":"message","id":"prediction-output","role":"assistant","content":[{"type":"output_text","text":"{\"suggestion\":\"Inspect the test result.\"}"}]}),
	));
	fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("fixture config");
	let mut session = NativeSession::start(&binary, home.path());
	let started=session.client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only","dynamicTools":[{"name":"parent_fixture_tool","description":"Inherited fixture tool","inputSchema":{"type":"object","properties":{}}}]})).await.expect("parent");
	let parent = started["thread"]["id"].as_str().expect("parent id").to_owned();
	session.client.turn_start(serde_json::json!({"threadId":parent,"input":[{"type":"text","text":"PARENT_CONTEXT_FOR_PREDICTION"}]})).await.expect("parent turn");
	loop {
		if let ServerEvent::Notification { method, params } =
			session.events.recv().await.expect("parent events")
			&& method == "turn/completed"
			&& params["threadId"] == parent
		{
			assert_eq!(params["turn"]["status"], "completed");
			break;
		}
	}
	let before = session
		.client
		.thread_read(serde_json::json!({"threadId":parent,"includeTurns":false}))
		.await
		.expect("parent metadata");
	for cancel in [true, false] {
		let guard =
			session.client.history_guard(session.client.history_revision()).expect("source guard");
		let prediction = session
			.client
			.fork_prediction_thread(&parent, guard)
			.await
			.expect("native prediction fork");
		let id = prediction.id().to_owned();
		if cancel {
			prediction.cancel().await.expect("cancel before inference");
			assert_eq!(calls.load(Ordering::SeqCst), 1);
		} else {
			let (_placeholder, empty) = mpsc::channel(1);
			let mut events = mem::replace(&mut session.events, empty);
			let (send, receive) = mpsc::channel(64);
			let (stop, mut stopped) = oneshot::channel::<()>();
			let forward = tokio::spawn(async move {
				loop {
					tokio::select! {
						_=&mut stopped=>break,
						event=events.recv()=>match event{Some(event)=>if send.send(event).await.is_err(){break;},None=>break,}
					}
				}
				events
			});
			let (_cancel, watch) = watch::channel(false);
			let schema = serde_json::json!({"type":"object","properties":{"suggestion":{"type":"string"}},"required":["suggestion"],"additionalProperties":false});
			let value = prediction
				.run("Suggest the next message without using tools.".into(), schema, receive, watch)
				.await
				.expect("prediction output");
			assert_eq!(
				serde_json::from_str::<Value>(&value).expect("structured output")["suggestion"],
				"Inspect the test result."
			);
			let _ = stop.send(());
			session.events = forward.await.expect("route cleanup");
		}
		let listed = session
			.client
			.request("thread/list", serde_json::json!({"limit":100}))
			.await
			.expect("history list");
		assert!(!listed["data"].as_array().expect("list rows").iter().any(|t| t["id"] == id));
		let loaded = session
			.client
			.request("thread/unsubscribe", serde_json::json!({"threadId":id}))
			.await
			.expect("subscription check");
		assert!(matches!(loaded["status"].as_str(), Some("notSubscribed" | "notLoaded")));
	}
	let after = session
		.client
		.thread_read(serde_json::json!({"threadId":parent,"includeTurns":false}))
		.await
		.expect("parent metadata");
	assert_eq!(before, after, "prediction must not alter parent metadata or receipt");
	assert_eq!(calls.load(Ordering::SeqCst), 2);
	let captured = bodies.lock().expect("captured requests");
	assert_eq!(captured[0]["model"], captured[1]["model"]);
	assert!(captured[1]["input"].to_string().contains("PARENT_CONTEXT_FOR_PREDICTION"));
	// Responses Lite carries the active tool catalog as an input item.
	let catalog = |body: &Value| {
		body["input"]
			.as_array()
			.expect("input items")
			.iter()
			.rfind(|item| item["type"] == "additional_tools")
			.cloned()
			.expect("native tool catalog")
	};
	let parent_tools = catalog(&captured[0]);
	let prediction_tools = catalog(&captured[1]);
	assert!(
		prediction_tools.to_string().contains("parent_fixture_tool"),
		"prediction inherits tools; never treat it as an isolated recap"
	);
	assert_eq!(parent_tools["tools"], prediction_tools["tools"]);

	backend.abort();
}
