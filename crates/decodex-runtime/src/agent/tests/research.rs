//! Isolated real coordinator/inbox tests. Browser records here are synthetic fixtures.
use std::{fs, future, iter, path::Path};

use futures_util::{SinkExt as _, StreamExt as _};
use rusqlite::Connection;
use serde_json::Value;
use tokio::{net::UnixListener, sync::oneshot, time};
use tokio_tungstenite::tungstenite::Message;

use crate::agent::{
	AgentCoordinator,
	research::{self, Events},
	tests,
};
use decodex_core::DecodexRoot;
use decodex_database::SqliteStore;
use decodex_pro_bridge::{ProBridge, Request};

fn complete_record(root: &Path, id: &str) {
	let bridge = ProBridge::new(Default::default(), root).unwrap();

	bridge
		.prepare(Request {
			id: id.into(),
			prompt: "Synthetic event qualification".into(),
			previous: None,
			project: None,
			context: None,
			delivery: Default::default(),
		})
		.unwrap();

	let path = root.join(format!("{id}.json"));
	let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();

	record["receipt"]["phase"] = serde_json::json!("complete");
	record["receipt"]["model"] = serde_json::json!("gpt-6-pro");
	record["receipt"]["execution"] =
		serde_json::json!({"messageId":"fixture-answer","model":"gpt-6-pro","mode":null});
	record["answer"] = serde_json::json!({"messageId":"fixture-answer","text":"Complete synthetic answer","codeBlocks":[],"links":[]});

	fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
}

async fn register(agent: &AgentCoordinator, id: &str) {
	let owner = agent.store.get_agent_work_item("agent".into()).await.unwrap();

	agent.observe_research_call(&serde_json::json!({"threadId":owner.codex_thread_id,"item":{"type":"mcpToolCall","server":"decodex_research","tool":"pro_research","arguments":{"id":id}}})).await.unwrap();
}

#[tokio::test]
async fn research_completion_queues_while_busy_and_wakes_once_after_repeated_recovery() {
	let (mut agent, mut sent, home) = tests::fixture().await;
	let root = home.path().join("research");

	agent.research_directory = Some(root.clone());

	agent.start_agent("agent", "Research a decision").await.unwrap();

	register(&agent, "busy-test").await;
	complete_record(&root, "busy-test");

	while sent.try_recv().is_ok() {}

	for _ in 0..3 {
		research::reconcile(&agent.store, &root, Some("busy-test".into()), false).await.unwrap();

		agent.wake_pending().await.unwrap();
	}

	assert!(sent.try_recv().is_err(), "A completion must not interrupt or steer a busy Agent");

	let events = agent.store.list_agent_wake_events("agent".into(), 100).await.unwrap();

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_kind, "research_completed");

	// A missed socket notification is recovered from the same persisted registration and receipt.
	let reopened = SqliteStore::open(
		&DecodexRoot::new(home.path().canonicalize().unwrap().join("root")).unwrap().paths(),
	)
	.unwrap();

	research::reconcile(&reopened, &root, None, true).await.unwrap();
	tests::complete(&mut agent, "agent").await;

	agent.wake_pending().await.unwrap();

	let starts: Vec<_> =
		iter::from_fn(|| sent.try_recv().ok()).filter(|r| r["method"] == "turn/start").collect();

	assert_eq!(starts.len(), 1, "Exactly one continuation on the original native conversation");
	assert_eq!(
		agent.store.list_agent_wake_events("agent".into(), 100).await.unwrap()[0].id,
		events[0].id
	);

	tests::complete(&mut agent, "agent").await;
	research::reconcile(&agent.store, &root, None, true).await.unwrap();

	agent.wake_pending().await.unwrap();

	assert!(
		!iter::from_fn(|| sent.try_recv().ok()).any(|r| r["method"] == "turn/start"),
		"Duplicate completion cannot create another wake"
	);
}

#[tokio::test]
async fn research_completion_respects_native_pause_and_dispatch_pause() {
	let goal = serde_json::json!({"threadId":"opaque thread/1","status":"paused","objective":"Paused research","tokenBudget":null,"tokensUsed":0,"timeUsedSeconds":0,"createdAt":1,"updatedAt":1});
	let (mut agent, mut sent, home) =
		tests::fixture_with_history(serde_json::json!({"_goal":goal})).await;
	let root = home.path().join("research");

	agent.research_directory = Some(root.clone());

	agent.start_agent("agent", "Research").await.unwrap();

	register(&agent, "pause-test").await;
	complete_record(&root, "pause-test");

	research::reconcile(&agent.store, &root, None, true).await.unwrap();
	tests::complete(&mut agent, "agent").await;

	while sent.try_recv().is_ok() {}

	agent.wake_pending().await.unwrap();

	assert!(!iter::from_fn(|| sent.try_recv().ok()).any(|r| r["method"] == "turn/start"));

	let pending = agent.store.list_agent_wake_events("agent".into(), 100).await.unwrap();

	assert_eq!(pending.len(), 1);
	assert!(pending[0].delivered_turn_id.is_none());

	agent.pause_dispatch(true);
	agent.wake_pending().await.unwrap();

	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn research_registration_does_not_follow_a_replaced_conversation() {
	let (mut agent, _sent, home) = tests::fixture().await;
	let root = home.path().join("research");

	agent.research_directory = Some(root.clone());

	agent.start_agent("agent", "Research").await.unwrap();

	register(&agent, "binding-test").await;
	complete_record(&root, "binding-test");

	Connection::open(home.path().join("root/server/decodex.sqlite3"))
		.unwrap()
		.execute("UPDATE agent_work_items SET codex_thread_id='replacement' WHERE id='agent'", [])
		.unwrap();
	research::reconcile(&agent.store, &root, None, true).await.unwrap();

	assert!(agent.store.list_agent_wake_events("agent".into(), 100).await.unwrap().is_empty());
}

#[tokio::test]
async fn research_read_before_completion_hint_does_not_wake_again() {
	let (mut agent, mut sent, home) = tests::fixture().await;
	let root = home.path().join("research");

	agent.research_directory = Some(root.clone());

	agent.start_agent("agent", "Research").await.unwrap();

	register(&agent, "early-read").await;
	complete_record(&root, "early-read");

	let owner = agent.store.get_agent_work_item("agent".into()).await.unwrap();
	let bridge = ProBridge::new(Default::default(), &root).unwrap();

	agent.observe_research_call(&serde_json::json!({"threadId":owner.codex_thread_id,"item":{"type":"mcpToolCall","server":"decodex_research","tool":"pro_result","status":"completed","arguments":{"id":"early-read"},"result":{"structuredContent":{"receipt":bridge.status("early-read").unwrap(),"answer":bridge.read("early-read").unwrap()}}}})).await.unwrap();

	research::reconcile(&agent.store, &root, None, false).await.unwrap();

	while sent.try_recv().is_ok() {}

	tests::complete(&mut agent, "agent").await;

	assert!(agent.store.list_agent_wake_events("agent".into(), 100).await.unwrap().is_empty());
	assert!(!iter::from_fn(|| sent.try_recv().ok()).any(|r| r["method"] == "turn/start"));
}

#[tokio::test]
async fn research_socket_reconnect_reads_missed_completion_and_acknowledges_full_result() {
	let (mut agent, mut sent, home) = tests::fixture().await;
	let root = home.path().join("research");

	agent.research_directory = Some(root.clone());

	agent.start_agent("agent", "Research").await.unwrap();

	register(&agent, "reconnect-test").await;

	ProBridge::new(Default::default(), &root).unwrap();

	let path = root.join("extension.sock");
	let listener = UnixListener::bind(&path).unwrap();
	let (close, closed) = oneshot::channel();
	let server = tokio::spawn(async move {
		let (stream, _) = listener.accept().await.unwrap();
		let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
		let msg = ws.next().await.unwrap().unwrap().into_text().unwrap();
		let req: Value = serde_json::from_str(&msg).unwrap();

		assert_eq!(req["method"], "Decodex.research.subscribe");

		ws.send(Message::Text(serde_json::json!({"id":req["id"],"result":{}}).to_string().into()))
			.await
			.unwrap();

		let _ = closed.await;
	});
	let mut events = Events::new(Some(root.clone()));

	assert!(
		time::timeout(std::time::Duration::from_secs(2), events.next()).await.unwrap().is_none()
	);

	close.send(()).unwrap();
	server.await.unwrap();

	fs::remove_file(&path).unwrap();

	complete_record(&root, "reconnect-test");

	let listener = UnixListener::bind(&path).unwrap();
	let server = tokio::spawn(async move {
		let (stream, _) = listener.accept().await.unwrap();
		let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
		let msg = ws.next().await.unwrap().unwrap().into_text().unwrap();
		let req: Value = serde_json::from_str(&msg).unwrap();

		ws.send(Message::Text(serde_json::json!({"id":req["id"],"result":{}}).to_string().into()))
			.await
			.unwrap();

		future::pending::<()>().await;
	});

	assert!(
		time::timeout(std::time::Duration::from_secs(3), events.next()).await.unwrap().is_none()
	);

	research::reconcile(&agent.store, &root, None, true).await.unwrap();

	while sent.try_recv().is_ok() {}

	tests::complete(&mut agent, "agent").await;

	agent.wake_pending().await.unwrap();

	assert_eq!(
		iter::from_fn(|| sent.try_recv().ok()).filter(|r| r["method"] == "turn/start").count(),
		1
	);

	let owner = agent.store.get_agent_work_item("agent".into()).await.unwrap();
	let bridge = ProBridge::new(Default::default(), &root).unwrap();

	agent.observe_research_call(&serde_json::json!({"threadId":owner.codex_thread_id,"item":{"type":"mcpToolCall","server":"decodex_research","tool":"pro_result","status":"completed","arguments":{"id":"reconnect-test"},"result":{"structuredContent":{"receipt":bridge.status("reconnect-test").unwrap(),"answer":bridge.read("reconnect-test").unwrap()}}}})).await.unwrap();

	assert!(
		agent.store.list_agent_wake_events("agent".into(), 100).await.unwrap().is_empty(),
		"Reading the full result consumes its notification, not Work assessment"
	);

	research::reconcile(&agent.store, &root, None, true).await.unwrap();

	assert!(agent.store.list_agent_wake_events("agent".into(), 100).await.unwrap().is_empty());

	server.abort();
}

#[tokio::test]
async fn research_failure_is_one_attention_event_instead_of_silent_waiting() {
	let (mut agent, _sent, home) = tests::fixture().await;
	let root = home.path().join("research");

	agent.research_directory = Some(root.clone());

	agent.start_agent("agent", "Research").await.unwrap();

	register(&agent, "attention-test").await;
	complete_record(&root, "attention-test");

	let path = root.join("attention-test.json");
	let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();

	record["receipt"]["phase"] = serde_json::json!("submitted");
	record["receipt"]["lastError"] = serde_json::json!("ChatGPT requires sign in");
	record["answer"] = Value::Null;

	fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();

	for _ in 0..2 {
		research::reconcile(&agent.store, &root, None, false).await.unwrap();
	}

	let events = agent.store.list_agent_wake_events("agent".into(), 100).await.unwrap();

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_kind, "research_attention");
	assert!(events[0].payload.contains("requires sign in"));
}
