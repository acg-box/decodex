//! Native model selection, inference and cold persistence through the retained bridge.
use super::*;
use decodex_codex::app_server_client::ThreadModelSelection;
#[path = "chief_process_native_model_store.rs"] mod journal;

const EFFORT: &str = "future-provider-reasoning-effort-over-32-bytes";

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native model selection qualification"]
async fn installed_model_selection_changes_next_inference_and_survives_restart() {
	tokio::time::timeout(Duration::from_secs(45), qualify(false)).await.expect("bounded fixture");
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated active model selection qualification"]
async fn installed_active_model_selection_preserves_admitted_inference() {
	tokio::time::timeout(Duration::from_secs(45), qualify(true)).await.expect("bounded fixture");
}

async fn qualify(running: bool) {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir_in("/tmp").expect("fixture home");
	let catalog = home.path().join("models.json");
	let models =
		[effort::fixture_model("fixture-a", EFFORT), effort::fixture_model("fixture-b", EFFORT)];
	std::fs::write(
		&catalog,
		serde_json::to_vec(&json!({"models":models})).expect("native model fixture operation"),
	)
	.expect("native model fixture operation");
	let listener =
		tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("native model fixture operation");
	let address = listener.local_addr().expect("native model fixture operation");
	let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
	let mut listener = Some(listener);
	let mut backend = None;
	if !running {
		backend = Some(start_backend(
			listener.take().expect("native model fixture operation"),
			requests.clone(),
			bodies.clone(),
		));
	}

	std::fs::write(home.path().join("config.toml"), format!("model=\"fixture-a\"\nmodel_catalog_json={}\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"OpenAI\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n", serde_json::to_string(&catalog).expect("native model fixture operation"))).expect("native model fixture operation");
	let mut session = NativeSession::start(&binary, home.path());
	let started = session.client.thread_start(json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only","model":"fixture-a","developerInstructions":"Keep fixture instructions"})).await.expect("native model fixture operation");
	let thread =
		started["thread"]["id"].as_str().expect("native model fixture operation").to_owned();
	let original = session.client.observed_task_models(&thread).expect("start hydration").0;
	assert_eq!(original.model, "fixture-a");
	let active_turn = if running {
		let result = session
			.client
			.turn_start(
				json!({"threadId":thread,"input":[{"type":"text","text":"Return fixture output"}]}),
			)
			.await
			.expect("native model fixture operation");
		Some(result["turn"]["id"].as_str().expect("native model fixture operation").to_owned())
	} else {
		run_turn(&mut session, &thread).await;
		None
	};
	let (store, attempt, reserved) =
		journal::reserve(home.path(), &session.client, &thread, active_turn.as_deref(), EFFORT)
			.await;
	let (_, guard) = session.client.configured_task_models(&thread).expect("configured settings");
	session
		.client
		.queue_thread_model_selection(
			&ThreadModelSelection::new(&thread, "fixture-b", Some(EFFORT.into()))
				.expect("native model fixture operation"),
			guard,
		)
		.await
		.expect("native model fixture operation");
	wait_selection(&mut session, &thread, "fixture-b").await;
	journal::observe(&store, &session.client, attempt, reserved).await;
	if let Some(turn) = active_turn {
		assert!(session.client.observed_task_models(&thread).is_none());
		backend = Some(start_backend(
			listener.take().expect("native model fixture operation"),
			requests.clone(),
			bodies.clone(),
		));
		finish_turn(&mut session, &thread, &turn).await;
	}
	assert_eq!(requests.load(Ordering::Acquire), 1, "settings do not start inference");
	let selected = session.client.observed_task_models(&thread).expect("published settings").0;
	assert_eq!(selected.model, "fixture-b");
	assert_eq!(selected.model_provider, original.model_provider);
	assert_eq!(selected.service_tier, original.service_tier);
	run_turn(&mut session, &thread).await;
	drop(session);
	let mut session = NativeSession::start(&binary, home.path());
	session
		.client
		.thread_resume(json!({"threadId":thread}))
		.await
		.expect("native model fixture operation");
	assert_eq!(session.client.observed_task_models(&thread).expect("resume hydration").0, selected);
	run_turn(&mut session, &thread).await;
	// Native omission preserves effort instead of selecting a local default.
	let (_, guard) =
		session.client.observed_task_models(&thread).expect("native model fixture operation");
	session
		.client
		.queue_thread_model_selection(
			&ThreadModelSelection::new(&thread, "fixture-a", None)
				.expect("native model fixture operation"),
			guard,
		)
		.await
		.expect("native model fixture operation");
	wait_selection(&mut session, &thread, "fixture-a").await;
	assert_eq!(
		session
			.client
			.observed_task_models(&thread)
			.expect("native model fixture operation")
			.0
			.effort
			.as_deref(),
		Some(EFFORT)
	);
	run_turn(&mut session, &thread).await;
	assert_eq!(requests.load(Ordering::Acquire), 4);
	assert_inference_sequence(&bodies);
	let backend = backend.expect("native model fixture operation");
	assert!(!backend.is_finished(), "fixture assertions passed");
	backend.abort();
}

async fn run_turn(session: &mut NativeSession, thread: &str) {
	let result = session
		.client
		.turn_start(
			json!({"threadId":thread,"input":[{"type":"text","text":"Return fixture output"}]}),
		)
		.await
		.expect("native model fixture operation");
	let turn = result["turn"]["id"].as_str().expect("native model fixture operation");
	finish_turn(session, thread, turn).await;
}

async fn finish_turn(session: &mut NativeSession, thread: &str, turn: &str) {
	loop {
		if let ServerEvent::Notification { method, params } =
			session.events.recv().await.expect("native model fixture operation")
			&& method == "turn/completed"
			&& params["threadId"] == thread
			&& params["turn"]["id"] == turn
		{
			assert_eq!(params["turn"]["status"], "completed");
			break;
		}
	}
}

async fn wait_selection(session: &mut NativeSession, thread: &str, model: &str) {
	loop {
		if let ServerEvent::Notification { method, params } =
			session.events.recv().await.expect("native model fixture operation")
			&& method == "thread/settings/updated"
			&& params["threadId"] == thread
			&& params["threadSettings"]["model"] == model
		{
			break;
		}
	}
}

fn start_backend(
	listener: tokio::net::TcpListener,
	requests: Arc<std::sync::atomic::AtomicUsize>,
	bodies: Arc<std::sync::Mutex<Vec<Value>>>,
) -> tokio::task::JoinHandle<()> {
	tokio::spawn(serve_fixture(
		listener,
		requests,
		None,
		Some(bodies),
		None,
		|serial| json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Done"}]}),
	))
}

fn assert_inference_sequence(bodies: &std::sync::Mutex<Vec<Value>>) {
	let bodies = bodies.lock().expect("native model fixture operation");
	assert_eq!(
		bodies
			.iter()
			.map(|body| body["model"].as_str().expect("native model fixture operation"))
			.collect::<Vec<_>>(),
		["fixture-a", "fixture-b", "fixture-b", "fixture-a"]
	);
	for body in bodies.iter() {
		assert_eq!(body["reasoning"]["effort"], EFFORT);
		assert!(
			body.to_string().contains("Keep fixture instructions"),
			"preserve native developer instructions"
		);
	}
}
