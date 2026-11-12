//! Native history, service recap state and isolated inference with no production provider.
use std::{
	env, fs, mem,
	sync::{Mutex, atomic::AtomicUsize},
};

use tokio::{
	net::TcpListener,
	sync::{mpsc, oneshot},
	time,
};

use crate::{
	account_launch::agent_process::native_tests::{
		self, Arc, Duration, NativeSession, Ordering, ServerEvent,
	},
	agent_recap::{self, Recaps},
	agent_usage_estimate::{Source, SourceKey},
};
use decodex_core::{AccountId, ProcessGenerationId};
use decodex_protocol::{EntityId, TaskRecapPhase};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native recap service"]
async fn installed_recap_uses_native_history_and_invalidates_after_new_input() {
	time::timeout(Duration::from_secs(60), async {
		for mode in ["legacy", "paginated"] {
			qualify(mode).await;
		}
	})
	.await
	.expect("bounded recap fixture");
}
async fn assert_recap_invalidated(recaps: &Recaps, source: &Source) {
	for _ in 0..100 {
		if recaps.status(EntityId::new("work").expect("id"), Some(source)).phase
			== TaskRecapPhase::Cancelled
		{
			break;
		}

		time::sleep(Duration::from_millis(10)).await;
	}

	assert_eq!(
		recaps.status(EntityId::new("work").expect("id"), Some(source)).phase,
		TaskRecapPhase::Cancelled
	);
}

async fn independent_recap(
	binary: &std::ffi::OsStr,
	home: &std::path::Path,
	source: &Source,
) -> NativeSession {
	let reader = NativeSession::start(binary, home);
	let external_source = Source { client: reader.client.clone(), key: source.key.clone() };
	assert!(reader.client.configured_task_permissions(&source.key.thread).is_none());
	let external = agent_recap::prepare(&external_source, None)
		.await
		.expect("cold read while another process owns the source");
	let temporary = reader
		.client
		.start_temporary_structured(external.options)
		.await
		.expect("independent recap thread");
	assert_ne!(temporary.id(), source.key.thread);
	let (_cancel, cancellation) = tokio::sync::watch::channel(false);
	let result = temporary
		.run_isolated(external.prompt, agent_recap::schema(), None, cancellation)
		.await
		.expect("recap does not need the source writer");
	assert!(agent_recap::parse(&result).is_some());
	assert!(reader.client.configured_task_permissions(&source.key.thread).is_none());
	reader
}

fn recap_source(session: &NativeSession, thread: &str) -> Source {
	Source {
		client: session.client.clone(),
		key: SourceKey {
			generation: ProcessGenerationId::new("10000000-0000-4000-8000-000000000001")
				.expect("id"),
			account: AccountId::new("20000000-0000-4000-8000-000000000002").expect("id"),
			work: "work".into(),
			thread: thread.into(),
			revision: 1,
			history_revision: session.client.history_revision(),
		},
	}
}

async fn qualify(mode: &str) {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native binary");
	let home = tempfile::tempdir().expect("fixture home");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("fixture listener");
	let address = listener.local_addr().expect("address");
	let calls = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		calls.clone(),
		None,
		Some(bodies.clone()),
		None,
		|serial| serde_json::json!({"type":"message","role":"assistant","id":format!("output-{serial}"),"content":[{"type":"output_text","text":if matches!(serial, 1 | 2) {r#"{"summary":"The fix was tested but is not installed.","next_action":null}"#} else {"The fix is tested, but not installed."}}]}),
	));

	fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("fixture config");

	let mut session = NativeSession::start(&binary, home.path());
	let started=session.client.thread_start(serde_json::json!({"cwd":home.path(),"model":"gpt-5.6-sol","approvalPolicy":"never","sandbox":"read-only","historyMode":mode})).await.expect("parent thread");
	let thread = started["thread"]["id"].as_str().expect("parent identity").to_owned();

	session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Fix the bug and validate it. Do not install."}]})).await.expect("initial turn");

	while let Some(event) = session.events.recv().await {
		if matches!(event,ServerEvent::Notification{method,params} if method=="turn/completed" && params["threadId"]==thread)
		{
			break;
		}
	}

	let source = recap_source(&session, &thread);
	let recaps = Recaps::default();
	let cancelled = recaps
		.start(
			Source { client: source.client.clone(), key: source.key.clone() },
			"recap-one",
			Default::default(),
		)
		.expect("recap start");
	let prepared = agent_recap::prepare(&source, None).await.expect("native public history");

	assert!(prepared.prompt.contains("Do not install"));
	assert!(prepared.prompt.contains("not installed"));

	let before = prepared.latest_turn;
	let temporary =
		session.client.start_temporary_structured(prepared.options).await.expect("isolated thread");
	let (stop, mut stopped) = oneshot::channel::<()>();
	let routing = recaps.clone();
	let (named, name_observed) = oneshot::channel();
	let mut named = Some(named);
	let (_placeholder, empty) = mpsc::channel(1);
	let mut events = mem::replace(&mut session.events, empty);
	let forward = tokio::spawn(async move {
		loop {
			tokio::select! {_=&mut stopped=>break,event=events.recv()=>match event {Some(event)=>{if matches!(&event,ServerEvent::Notification {method,..} if method=="thread/name/updated") && let Some(named)=named.take() { let _=named.send(()); } routing.observe(&event);},None=>break,}}
		}

		events
	});
	let result = temporary
		.run_isolated(prepared.prompt, agent_recap::schema(), None, cancelled)
		.await
		.expect("native recap inference");

	recaps.finish("work", "recap-one", agent_recap::parse(&result));

	let (_, settings_guard) =
		session.client.configured_task_permissions(&thread).expect("source permissions");
	let title_guard = session
		.client
		.with_thread_name_guard(&thread, settings_guard.clone())
		.expect("title source");
	session
		.client
		.name_thread_guarded(&thread, "Automatic topic title", title_guard)
		.await
		.expect("native title update");
	time::timeout(Duration::from_secs(2), name_observed)
		.await
		.expect("name publication")
		.expect("observed native name");
	assert!(settings_guard.is_live(), "title updates preserve recap and permission state");

	assert_eq!(
		recaps.status(EntityId::new("work").expect("id"), Some(&source)).phase,
		TaskRecapPhase::Ready
	);
	assert_eq!(
		session.client.thread_latest_turn_id(&thread).await.expect("unchanged parent history"),
		before
	);
	assert_eq!(calls.load(Ordering::Acquire), 2);

	{
		let captured = bodies.lock().expect("request bodies");

		assert!(captured[1]["tools"].as_array().is_none_or(Vec::is_empty));
	}

	// The source stays loaded in its original process, which retains its writer
	// lock. Another process must summarize it using reads and its own temporary
	// thread, without resuming or acquiring the source writer.
	let _reader = independent_recap(&binary, home.path(), &source).await;
	assert_eq!(
		session.client.thread_latest_turn_id(&thread).await.expect("source remains unchanged"),
		before
	);
	assert_eq!(calls.load(Ordering::Acquire), 3);
	assert!(
		bodies.lock().expect("captured native requests")[2]["tools"]
			.as_array()
			.is_none_or(Vec::is_empty)
	);

	session
		.client
		.turn_start(
			serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"New correction"}]}),
		)
		.await
		.expect("new input");

	assert_recap_invalidated(&recaps, &source).await;

	let _ = stop.send(());

	session.events = forward.await.expect("route owner");

	backend.abort();
}
