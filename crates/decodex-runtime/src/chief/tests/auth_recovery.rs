use super::*;

#[tokio::test]
async fn auth_recovery_receipts_survive_reopen_without_retry_or_completion() {
	let (mut chief, mut sent, directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let (io, mut write) = tokio::io::duplex(16384);
	let (read, writer) = tokio::io::split(io);
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
		let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
			.await
			.unwrap()
			.unwrap();
		chief.handle_event(event).await.unwrap();
	}
	for (field, value) in [
		("threadId", json!("foreign")),
		("turnId", json!("old")),
		("provider", json!(false)),
		("message", json!("x".repeat(4097))),
	] {
		let mut invalid = params.clone();
		invalid[field] = value;
		chief
			.handle_event(ServerEvent::Notification {
				method: "modelProvider/authRecoveryCompleted".into(),
				params: invalid,
			})
			.await
			.unwrap();
	}
	let store = chief.store.clone();
	let work = store.get_chief_work_item("chief".into()).await.unwrap();
	assert_eq!(work.dispatch_state, decodex_database::ChiefDispatchState::Running);
	let rows = store.read_chief_work_events("chief".into(), 32).await.unwrap();
	let receipts: Vec<_> =
		rows.iter().filter(|row| row.event_kind.starts_with("auth_recovery_")).collect();
	assert_eq!(receipts.len(), 3);
	assert_eq!(receipts[1].event_kind, "auth_recovery_completed");
	assert!(receipts.iter().all(|row| row.disposition.is_some()
		&& row.delivered_turn_id.as_deref() == Some("opaque turn/1")));
	let projected = crate::application::render_chief_history_for_auth_test(rows);
	assert_eq!(projected.len(), 3);
	assert!(projected.iter().all(|row| row.kind == "auth_recovery"
		&& row.text.contains("Saved event")
		&& !row.text.contains("Disposition:")));
	assert!(projected[1].text.contains("succeeded"));
	assert!(projected[2].text.contains("started"));
	assert!(store.list_pending_chief_events(32).await.unwrap().is_empty());
	assert!(store.list_chief_wake_events("chief".into(), 32).await.unwrap().is_empty());
	chief.wake_pending().await.unwrap();
	assert!(sent.try_recv().is_err());
	// A lost connection cannot manufacture recovery success from an earlier start.
	drop(write);
	drop(chief);
	drop(store);
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let reopened = SqliteStore::open(&root.paths()).unwrap();
	let rows = reopened.read_chief_work_events("chief".into(), 32).await.unwrap();
	let after = crate::application::render_chief_history_for_auth_test(rows);
	assert_eq!(serde_json::to_value(after).unwrap(), serde_json::to_value(projected).unwrap());
	assert!(sent.try_recv().is_err());
}
