//! Real overload, durable selection, cold resume and same-model retry through the native bridge.
use std::{env, fs, sync::atomic::AtomicUsize};

use tokio::{net::TcpListener, sync::mpsc::Receiver, time};

use crate::{
	account_launch::agent_process::native_tests::{
		self, Arc, Duration, NativeSession, Ordering, ServerEvent, Value,
	},
	agent::{AgentConfig, AgentCoordinator},
};
use decodex_core::DecodexRoot;
use decodex_database::{EnqueueAgentEvent, SqliteStore};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native capacity recovery"]
async fn installed_capacity_retry_retains_selected_model_after_process_and_store_restart() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().expect("isolated native home");
	let home_path = home.path().canonicalize().expect("canonical home");
	let root = DecodexRoot::new(home_path.join("product")).expect("isolated product");

	root.paths().ensure_layout().expect("product layout");

	let listener = TcpListener::bind("127.0.0.1:0").await.expect("fixture backend");
	let address = listener.local_addr().expect("backend address");
	let count = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
	let backend = tokio::spawn(native_tests::serve_fixture_frames(
		listener,
		Arc::clone(&count),
		None,
		Some(Arc::clone(&bodies)),
		|n| {
			if n == 0 {
				return vec![
					serde_json::json!({"type":"response.failed","response":{"id":"overload","error":{"code":"server_is_overloaded","message":"Selected model is at capacity."}}}),
				];
			}

			vec![
				serde_json::json!({"type":"response.created","response":{"id":"recovered"}}),
				serde_json::json!({"type":"response.output_item.done","item":{"type":"message","role":"assistant","id":"answer","content":[{"type":"output_text","text":"Done."}]}}),
				serde_json::json!({"type":"response.completed","response":{"id":"recovered","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}),
			]
		},
	));

	fs::write(home_path.join("config.toml"),format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\n[model_providers.fixture]\nname = \"Capacity fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).expect("native fixture config");

	let config =
		AgentConfig::new("gpt-5.6-sol".into(), "high".into(), home_path.display().to_string());
	let (mut session, store) = (
		NativeSession::start(&binary, &home_path),
		SqliteStore::open(&root.paths()).expect("product store"),
	);
	let mut agent = AgentCoordinator::new(store.clone(), session.client.clone(), config.clone())
		.expect("coordinator");

	AgentCoordinator::reserve_root(&store, "agent", "capacity fixture")
		.await
		.expect("reserve root");

	store.enqueue_agent_event(EnqueueAgentEvent {source_event_id:"capacity-user-input".into(),work_item_id:"agent".into(),event_kind:"user_message".into(),payload:serde_json::json!({"text":"capacity-fixture-input","options":{"execution":{"model":"gpt-5.6-terra","reasoning_effort":"medium","fast":false},"attachments":[]}}).to_string()}).await.expect("selected input");

	time::timeout(Duration::from_secs(30), async {
		agent.wake_pending().await.expect("selected turn");

		finish(&mut agent, &mut session.events, "failed").await;
	})
	.await
	.expect("native failure deadline");

	let retry = store
		.pending_agent_capacity_retry("agent".into())
		.await
		.expect("retry read")
		.expect("native overload retry");

	assert_eq!(count.load(Ordering::Acquire), 1);

	drop(agent);
	drop(store);
	drop(session);

	let store = SqliteStore::open(&root.paths()).expect("reopened store");
	let mut session = NativeSession::start(&binary, &home_path);
	let mut agent = AgentCoordinator::new(store.clone(), session.client.clone(), config)
		.expect("reopened coordinator");

	time::timeout(Duration::from_secs(30), async {
		agent.check_due_followups(retry.due_at_micros).await.expect("saved retry");

		finish(&mut agent, &mut session.events, "completed").await;
	})
	.await
	.expect("native retry deadline");

	assert!(
		store
			.pending_agent_capacity_retry("agent".into())
			.await
			.expect("final retry read")
			.is_none()
	);
	assert_eq!(count.load(Ordering::Acquire), 2);

	{
		let requests = bodies.lock().expect("captured requests");

		for request in requests.iter() {
			assert_eq!(request["model"], "gpt-5.6-terra");
			assert_eq!(request["reasoning"]["effort"], "medium");
		}

		assert_eq!(
			requests[1]["input"].to_string().matches("capacity-fixture-input").count(),
			1,
			"retry must retain existing input, not duplicate it"
		);
	}

	effort_only_followup(&mut agent, &store, &mut session.events, &bodies).await;
	drop(agent);
	drop(store);
	drop(session);

	backend.abort();
}

async fn effort_only_followup(
	agent: &mut AgentCoordinator,
	store: &SqliteStore,
	events: &mut Receiver<ServerEvent>,
	bodies: &std::sync::Mutex<Vec<Value>>,
) {
	store.enqueue_agent_event(EnqueueAgentEvent {source_event_id:"effort-only-after-recovery".into(),work_item_id:"agent".into(),event_kind:"user_message".into(),payload:serde_json::json!({"text":"next-fixture-input","options":{"execution":{"reasoning_effort":"low"},"attachments":[]}}).to_string()}).await.expect("partial user choice");

	time::timeout(Duration::from_secs(30), async {
		agent.wake_pending().await.expect("partial selection dispatch");

		finish(agent, events, "completed").await;
	})
	.await
	.expect("partial selection deadline");

	let requests = bodies.lock().expect("captured partial request");

	assert_eq!(requests.len(), 3);
	assert_eq!(requests[2]["model"], "gpt-5.6-terra");
	assert_eq!(requests[2]["reasoning"]["effort"], "low");
}

async fn finish(agent: &mut AgentCoordinator, events: &mut Receiver<ServerEvent>, expected: &str) {
	loop {
		let event = events.recv().await.expect("native events");
		let terminal = if let ServerEvent::Notification { method, params } = &event {
			if method == "turn/completed" {
				assert_eq!(params["turn"]["status"], expected);

				if expected == "failed" {
					assert_eq!(params["turn"]["error"]["codexErrorInfo"], "serverOverloaded");
				}

				true
			} else {
				false
			}
		} else {
			false
		};

		agent.handle_event(event).await.expect("native event reduction");

		if terminal {
			break;
		}
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; native throttling and quota classification"]
async fn installed_throttling_and_quota_do_not_schedule_capacity_retries() {
	for (code, expected, requests) in [
		("slow_down", "rateLimitExceeded", 2),
		("credit_balance_exhausted", "usageLimitExceeded", 1),
		("organization_spend_limit_exceeded", "usageLimitExceeded", 1),
		("project_spend_limit_exceeded", "usageLimitExceeded", 1),
	] {
		native_error_classification(code, expected, requests).await;
	}
}

async fn native_error_classification(code: &'static str, expected: &str, requests: usize) {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().expect("isolated home");
	let path = home.path().canonicalize().expect("canonical home");
	let root = DecodexRoot::new(path.join("product")).expect("product root");

	root.paths().ensure_layout().expect("layout");

	let listener = TcpListener::bind("127.0.0.1:0").await.expect("backend");
	let address = listener.local_addr().expect("address");
	let count = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(native_tests::serve_fixture_frames(
		listener,
		Arc::clone(&count),
		None,
		None,
		move |_| {
			vec![
				serde_json::json!({"type":"response.failed","response":{"id":"limited","error":{"code":code,"message":"Please try again in 0.01s."}}}),
			]
		},
	));

	fs::write(path.join("config.toml"),format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\n[model_providers.fixture]\nname = \"Native classification fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\nstream_max_retries = 1\nrequest_max_retries = 0\n")).expect("config");

	let config =
		AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), path.display().to_string());
	let (mut session, store) =
		(NativeSession::start(&binary, &path), SqliteStore::open(&root.paths()).expect("store"));
	let mut agent =
		AgentCoordinator::new(store.clone(), session.client.clone(), config).expect("coordinator");

	AgentCoordinator::reserve_root(&store, "agent", "Classification fixture").await.expect("root");

	store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "input".into(),
			work_item_id: "agent".into(),
			event_kind: "user_message".into(),
			payload: serde_json::json!({"text":"Return fixture answer."}).to_string(),
		})
		.await
		.expect("input");

	time::timeout(Duration::from_secs(30), async {
		agent.wake_pending().await.expect("start");

		loop {
			let event = session.events.recv().await.expect("event");
			let terminal = if let ServerEvent::Notification { method, params } = &event {
				if method == "turn/completed" {
					assert_eq!(params["turn"]["status"], "failed");
					assert_eq!(params["turn"]["error"]["codexErrorInfo"], expected, "code={code}");

					true
				} else {
					false
				}
			} else {
				false
			};

			agent.handle_event(event).await.expect("reduce");

			if terminal {
				break;
			}
		}
	})
	.await
	.expect("terminal deadline");

	assert_eq!(count.load(Ordering::Acquire), requests, "code={code}");
	assert!(
		store.pending_agent_capacity_retry("agent".into()).await.expect("retry read").is_none()
	);

	drop(agent);
	drop(session);

	let reopened = SqliteStore::open(&root.paths()).expect("reopen");

	assert!(
		reopened
			.pending_agent_capacity_retry("agent".into())
			.await
			.expect("cold retry read")
			.is_none()
	);

	backend.abort();
}
