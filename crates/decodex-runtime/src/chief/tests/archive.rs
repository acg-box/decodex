use super::*;

#[tokio::test]
async fn restore_preserves_identity_and_pending_input_without_creating_a_turn() {
	let (mut chief, mut sent, _directory) = fixture_with_history(json!({"_archived":true})).await;
	chief.start_chief("chief", "Initial").await.unwrap();
	chief.handle_event(ServerEvent::Notification {method:"turn/completed".into(),params:json!({"threadId":"opaque thread/1","turn":{"id":"opaque turn/1","status":"completed","items":[]}})}).await.unwrap();
	chief
		.store
		.enqueue_chief_event(EnqueueChiefEvent {
			source_event_id: "queued".into(),
			work_item_id: "chief".into(),
			event_kind: "user_message".into(),
			payload: json!({"text":"Keep this input"}).to_string(),
		})
		.await
		.unwrap();
	while sent.try_recv().is_ok() {}
	assert!(matches!(
		chief.restore_archived_thread("chief", "foreign").await,
		Err(ChiefError::Rejected(_))
	));
	assert!(sent.try_recv().is_err());
	chief.restore_archived_thread("chief", "opaque thread/1").await.unwrap();
	chief.restore_archived_thread("chief", "opaque thread/1").await.unwrap();
	let mut mutations = 0;
	while let Ok(request) = sent.try_recv() {
		assert!(
			["thread/read", "thread/list", "thread/unarchive"]
				.contains(&request["method"].as_str().unwrap())
		);
		mutations += usize::from(request["method"] == "thread/unarchive");
	}
	assert_eq!(mutations, 1);
	let work = chief.store.get_chief_work_item("chief".into()).await.unwrap();
	assert_eq!(work.codex_thread_id.as_deref(), Some("opaque thread/1"));
	assert_eq!(work.dispatch_state, decodex_database::ChiefDispatchState::Idle);
	let pending = chief.store.list_chief_wake_events("chief".into(), 32).await.unwrap();
	assert!(
		pending
			.iter()
			.any(|event| event.source_event_id == "queued" && event.disposition.is_none())
	);
}

#[tokio::test]
async fn restore_distinguishes_rejection_disconnect_and_another_clients_success() {
	for (settings, expected) in [
		(json!({"_archived":true,"_archive_reject":true}), "rejected"),
		(json!({"_archived":true,"_archive_disconnect":true}), "unknown"),
		(json!({"_archived":true,"_archive_reject":true,"_archive_peer_restored":true}), "active"),
	] {
		let (mut chief, mut sent, _directory) = fixture_with_history(settings).await;
		chief.start_chief("chief", "Initial").await.unwrap();
		while sent.try_recv().is_ok() {}
		let result = chief.restore_archived_thread("chief", "opaque thread/1").await;
		match expected {
			"rejected" => assert!(matches!(result, Err(ChiefError::Rejected(_)))),
			"unknown" => assert!(matches!(result, Err(ChiefError::UnknownDispatch))),
			_ => assert!(result.is_ok()),
		}
		let mut mutations = 0;
		while let Ok(request) = sent.try_recv() {
			assert_ne!(request["method"], "turn/start");
			mutations += usize::from(request["method"] == "thread/unarchive");
		}
		assert_eq!(mutations, 1);
	}
}

#[tokio::test]
async fn restoration_reconciles_only_exact_positive_terminal_history_after_reopen() {
	let history = json!({"_archived":true,"opaque thread/1":{"thread":{"id":"opaque thread/1","status":{"type":"notLoaded"},"turns":[{"id":"opaque turn/1","status":"interrupted","items":[]}]}}});
	let (mut chief, mut sent, directory) = fixture_with_history(history).await;
	chief.start_chief("chief", "Initial").await.unwrap();
	chief.store.mark_chief_dispatch_unknown("chief".into()).await.unwrap();
	// Reopen the durable owner while retaining the fixture transport; native state
	// still owns the archive flag, and exact saved turn identity owns recovery.
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	chief.store = SqliteStore::open(&root.paths()).unwrap();
	while sent.try_recv().is_ok() {}
	chief.restore_archived_thread("chief", "opaque thread/1").await.unwrap();
	let work = chief.store.get_chief_work_item("chief".into()).await.unwrap();
	assert_eq!(work.dispatch_state, decodex_database::ChiefDispatchState::Idle);
	assert!(work.active_turn_id.is_none());
	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start");
		assert_ne!(request["method"], "thread/start");
	}
}

#[test]
fn archived_resume_diagnostic_is_bound_to_the_exact_thread_and_never_authorizes_restore() {
	let error = || {
		ClientError::Remote(decodex_codex::app_server_client::RpcError {
			code: -32600,
			message:
				"session target is archived. Run `codex unarchive target` to unarchive it first."
					.into(),
			data: None,
		})
	};
	assert!(matches!(super::super::resume_error(error(), "target"), ChiefError::ThreadArchived));
	assert!(matches!(super::super::resume_error(error(), "other"), ChiefError::Transport(_)));
}
