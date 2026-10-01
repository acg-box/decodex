use std::time::Duration;

use rusqlite::Connection;
use tokio::{io, time};

use crate::{agent::tests::*, application};
use decodex_core::DecodexRoot;

#[tokio::test]
async fn retired_auth_recovery_ignores_notifications_and_preserves_saved_history() {
	let (mut agent, mut sent, directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let (io, mut write) = io::duplex(16_384);
	let (read, writer) = io::split(io);
	let (_client, mut events) = AppServerClient::from_io(read, writer);
	let params = json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","provider":"AWS","message":"[Sign in](https://example.invalid)"});

	for completed in [false, true, false] {
		let method = if completed {
			"modelProvider/authRecoveryCompleted"
		} else {
			"modelProvider/authRecoveryStarted"
		};
		let wire = json!({"method":method,"params":params});

		write.write_all(format!("{wire}\n").as_bytes()).await.unwrap();

		let event = time::timeout(Duration::from_secs(2), events.recv()).await.unwrap().unwrap();

		agent.handle_event(event).await.unwrap();
	}

	let store = agent.store.clone();
	let work = store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Running);

	let rows = store.read_agent_work_events("agent".into(), 32).await.unwrap();
	let receipts: Vec<_> =
		rows.iter().filter(|row| row.event_kind.starts_with("auth_recovery_")).collect();

	assert!(receipts.is_empty());
	// Load a receipt saved before retirement. No production writer remains.
	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();

	{
		let connection = Connection::open(root.paths().product_database_file()).unwrap();

		connection.execute(
			"INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros,delivery_work_item_id,delivered_turn_id) VALUES('legacy-auth-recovery','agent','auth_recovery_started',?1,1,'resolved','Historical provider notice',1,'agent','opaque turn/1')",
			[params.to_string()],
		).unwrap();
	}

	let (transcript, _) = store.read_agent_transcript("agent".into(), None, 32).await.unwrap();
	let projected = application::render_agent_history_for_test(transcript)
		.into_iter()
		.filter(|entry| entry.kind == "auth_recovery")
		.collect::<Vec<_>>();

	assert_eq!(projected.len(), 1);
	assert!(projected.iter().all(|row| row.kind == "auth_recovery"
		&& row.text.contains("Saved event")
		&& !row.text.contains("Disposition:")));
	assert!(projected[0].text.contains("started"));
	assert!(store.list_pending_agent_events(32).await.unwrap().is_empty());
	assert!(store.list_agent_wake_events("agent".into(), 32).await.unwrap().is_empty());

	agent.wake_pending().await.unwrap();

	assert!(sent.try_recv().is_err());
	// A lost connection cannot manufacture recovery success from an earlier start.
	drop(write);
	drop(agent);
	drop(store);

	let reopened = SqliteStore::open(&root.paths()).unwrap();
	let (rows, _) = reopened.read_agent_transcript("agent".into(), None, 32).await.unwrap();
	let after = application::render_agent_history_for_test(rows)
		.into_iter()
		.filter(|entry| entry.kind == "auth_recovery")
		.collect::<Vec<_>>();

	assert_eq!(serde_json::to_value(after).unwrap(), serde_json::to_value(projected).unwrap());
	assert!(sent.try_recv().is_err());
}
