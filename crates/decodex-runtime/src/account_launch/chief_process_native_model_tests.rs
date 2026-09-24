//! Native model selection, inference and cold persistence through the retained bridge.
use super::*;
use decodex_codex::app_server_client::ThreadModelSelection;
#[path = "chief_process_native_model_store.rs"] mod journal;

const EFFORT: &str = "future-provider-reasoning-effort-over-32-bytes";

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native model selection qualification"]
async fn installed_model_selection_changes_next_inference_and_survives_restart() {
	tokio::time::timeout(Duration::from_secs(45), qualify(false, false, false))
		.await
		.expect("bounded fixture");
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated active model selection qualification"]
async fn installed_active_model_selection_preserves_admitted_inference() {
	tokio::time::timeout(Duration::from_secs(45), qualify(true, false, false))
		.await
		.expect("bounded fixture");
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated model without effort choices"]
async fn installed_model_without_effort_choices_remains_selectable() {
	tokio::time::timeout(Duration::from_secs(45), qualify(false, true, false))
		.await
		.expect("bounded fixture");
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; preserve explicit effort on a model with no effort choices"]
async fn installed_model_without_effort_choices_preserves_explicit_configuration() {
	tokio::time::timeout(Duration::from_secs(45), qualify(false, true, true))
		.await
		.expect("bounded fixture");
}

async fn qualify(running: bool, no_effort_choices: bool, explicit_effort: bool) {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir_in("/tmp").expect("fixture home");
	let catalog = home.path().join("models.json");
	write_catalog(&catalog, no_effort_choices);
	let selected_effort = (!no_effort_choices).then_some(EFFORT);
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

	let configured_effort = if explicit_effort {
		format!("model_reasoning_effort={}\n", json!(EFFORT))
	} else {
		String::new()
	};
	std::fs::write(home.path().join("config.toml"), format!("model=\"fixture-a\"\n{configured_effort}model_catalog_json={}\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"OpenAI\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n", serde_json::to_string(&catalog).expect("native model fixture operation"))).expect("native model fixture operation");
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
	let (store, attempt, reserved) = journal::reserve(
		home.path(),
		&session.client,
		&thread,
		active_turn.as_deref(),
		selected_effort,
	)
	.await;
	reviewer::select_task_model(
		&session.client,
		home.path(),
		&thread,
		"fixture-b",
		selected_effort,
	)
	.await;
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
	qualify_independent_task(&mut session, home.path()).await;
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
		selected_effort.or(explicit_effort.then_some(EFFORT))
	);
	run_turn(&mut session, &thread).await;
	assert_eq!(requests.load(Ordering::Acquire), 5);
	assert_inference_sequence(&bodies, no_effort_choices, explicit_effort);
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

fn assert_inference_sequence(
	bodies: &std::sync::Mutex<Vec<Value>>,
	no_effort_choices: bool,
	explicit_effort: bool,
) {
	let bodies = bodies.lock().expect("native model fixture operation");
	assert_eq!(
		bodies
			.iter()
			.map(|body| body["model"].as_str().expect("native model fixture operation"))
			.collect::<Vec<_>>(),
		["fixture-a", "fixture-b", "fixture-a", "fixture-b", "fixture-a"]
	);
	for body in bodies.iter() {
		assert_eq!(
			body["reasoning"]["effort"].as_str(),
			if no_effort_choices && !explicit_effort && body["model"] == "fixture-b" {
				None
			} else {
				Some(EFFORT)
			}
		);
		assert!(
			body.to_string().contains("Keep fixture instructions"),
			"preserve native developer instructions"
		);
	}
}

fn write_catalog(catalog: &std::path::Path, no_effort_choices: bool) {
	let mut target = effort::fixture_model("fixture-b", EFFORT);
	if no_effort_choices {
		target["supported_reasoning_levels"] = json!([]);
		target["default_reasoning_level"] = Value::Null;
	}
	let models = [effort::fixture_model("fixture-a", EFFORT), target];
	std::fs::write(
		catalog,
		serde_json::to_vec(&json!({"models":models})).expect("native model fixture operation"),
	)
	.expect("native model fixture operation");
}

async fn qualify_independent_task(session: &mut NativeSession, home: &std::path::Path) {
	let started=session.client.thread_start(json!({"cwd":home,"approvalPolicy":"never","sandbox":"read-only","developerInstructions":"Keep fixture instructions"})).await.expect("independent task");
	let thread = started["thread"]["id"].as_str().expect("independent thread");
	assert_eq!(
		session.client.configured_task_models(thread).expect("creation default").0.model,
		"fixture-a"
	);
	run_turn(session, thread).await;
}
