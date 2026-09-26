//! Native history, service recap state and isolated inference with no production provider.
use super::*;
use crate::chief_recap::{self, Recaps};
use decodex_core::{AccountId, ProcessGenerationId};
use decodex_protocol::{EntityId, TaskRecapPhase};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native recap service"]
async fn installed_recap_uses_native_history_and_invalidates_after_new_input() {
	tokio::time::timeout(Duration::from_secs(60), async {
		for mode in ["legacy", "paginated"] {
			qualify(mode).await;
		}
	})
	.await
	.expect("bounded recap fixture");
}
async fn qualify(mode: &str) {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native binary");
	let home = tempfile::tempdir().expect("fixture home");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("fixture listener");
	let address = listener.local_addr().expect("address");
	let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
	let backend = tokio::spawn(serve_fixture(
		listener,
		calls.clone(),
		None,
		Some(bodies.clone()),
		None,
		|serial| json!({"type":"message","role":"assistant","id":format!("output-{serial}"),"content":[{"type":"output_text","text":if serial==1 {r#"{"summary":"The fix was tested but is not installed.","next_action":null}"#} else {"The fix is tested, but not installed."}}]}),
	));
	std::fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("fixture config");
	let mut session = NativeSession::start(&binary, home.path());
	let started=session.client.thread_start(json!({"cwd":home.path(),"model":"gpt-5.6-sol","approvalPolicy":"never","sandbox":"read-only","historyMode":mode})).await.expect("parent thread");
	let thread = started["thread"]["id"].as_str().expect("parent identity").to_owned();
	session.client.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Fix the bug and validate it. Do not install."}]})).await.expect("initial turn");
	while let Some(event) = session.events.recv().await {
		if matches!(event,ServerEvent::Notification{method,params} if method=="turn/completed" && params["threadId"]==thread)
		{
			break;
		}
	}
	let source = crate::chief_usage_estimate::Source {
		client: session.client.clone(),
		key: crate::chief_usage_estimate::SourceKey {
			generation: ProcessGenerationId::new("10000000-0000-4000-8000-000000000001")
				.expect("id"),
			account: AccountId::new("20000000-0000-4000-8000-000000000002").expect("id"),
			work: "work".into(),
			thread: thread.clone(),
			revision: 1,
			history_revision: session.client.history_revision(),
		},
	};
	let recaps = Recaps::default();
	let cancelled = recaps
		.start(
			crate::chief_usage_estimate::Source {
				client: source.client.clone(),
				key: source.key.clone(),
			},
			"recap-one",
			Default::default(),
		)
		.expect("recap start");
	let prepared = chief_recap::prepare(&source, None).await.expect("native public history");
	assert!(prepared.prompt.contains("Do not install"));
	assert!(prepared.prompt.contains("not installed"));
	let before = prepared.latest_turn;
	let temporary =
		session.client.start_temporary_structured(prepared.options).await.expect("isolated thread");
	let temporary_id = temporary.id().to_owned();
	let private = recaps.register(&temporary_id).expect("route");
	let (stop, mut stopped) = tokio::sync::oneshot::channel::<()>();
	let routing = recaps.clone();
	let (_placeholder, empty) = tokio::sync::mpsc::channel(1);
	let mut events = std::mem::replace(&mut session.events, empty);
	let forward = tokio::spawn(async move {
		loop {
			tokio::select! {_=&mut stopped=>break,event=events.recv()=>match event {Some(event)=>{let _=routing.route(event);},None=>break,}}
		}
		events
	});
	let result = temporary
		.run(prepared.prompt, chief_recap::schema(), None, private, cancelled)
		.await
		.expect("native recap inference");
	recaps.finish("work", "recap-one", Some(&temporary_id), chief_recap::parse(&result));
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
	session
		.client
		.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"New correction"}]}))
		.await
		.expect("new input");
	for _ in 0..100 {
		if recaps.status(EntityId::new("work").expect("id"), Some(&source)).phase
			== TaskRecapPhase::Cancelled
		{
			break;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	assert_eq!(
		recaps.status(EntityId::new("work").expect("id"), Some(&source)).phase,
		TaskRecapPhase::Cancelled
	);
	let _ = stop.send(());
	session.events = forward.await.expect("route owner");
	backend.abort();
}
