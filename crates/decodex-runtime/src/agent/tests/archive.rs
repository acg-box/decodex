use crate::agent::tests::*;
use decodex_codex::app_server_client::RpcError;
use decodex_core::DecodexRoot;

#[test]
fn archived_resume_diagnostic_is_bound_to_the_exact_thread_and_never_authorizes_restore() {
	let error = || {
		ClientError::Remote(RpcError {
			code: -32_600,
			message:
				"session target is archived. Run `codex unarchive target` to unarchive it first."
					.into(),
			data: None,
		})
	};

	assert!(matches!(super::super::resume_error(error(), "target"), AgentError::ThreadArchived));
	assert!(matches!(super::super::resume_error(error(), "other"), AgentError::Transport(_)));
}

#[tokio::test]
async fn restore_preserves_identity_and_pending_input_without_creating_a_turn() {
	let (mut agent, mut sent, _directory) = fixture_with_history(json!({"_archived":true})).await;

	agent.start_agent("agent", "Initial").await.unwrap();
	agent.handle_event(ServerEvent::Notification {method:"turn/completed".into(),params:json!({"threadId":"opaque thread/1","turn":{"id":"opaque turn/1","status":"completed","items":[]}})}).await.unwrap();
	agent
		.store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "queued".into(),
			work_item_id: "agent".into(),
			event_kind: "user_message".into(),
			payload: json!({"text":"Keep this input"}).to_string(),
		})
		.await
		.unwrap();

	while sent.try_recv().is_ok() {}

	assert!(matches!(
		agent.restore_archived_thread("agent", "foreign").await,
		Err(AgentError::Rejected(_))
	));
	assert!(sent.try_recv().is_err());

	agent.restore_archived_thread("agent", "opaque thread/1").await.unwrap();
	agent.restore_archived_thread("agent", "opaque thread/1").await.unwrap();

	let mut mutations = 0;

	while let Ok(request) = sent.try_recv() {
		assert!(
			["thread/read", "thread/list", "thread/unarchive"]
				.contains(&request["method"].as_str().unwrap())
		);

		mutations += usize::from(request["method"] == "thread/unarchive");
	}

	assert_eq!(mutations, 1);

	let work = agent.store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(work.codex_thread_id.as_deref(), Some("opaque thread/1"));
	assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Idle);

	let pending = agent.store.list_agent_wake_events("agent".into(), 32).await.unwrap();

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
		let (mut agent, mut sent, _directory) = fixture_with_history(settings).await;

		agent.start_agent("agent", "Initial").await.unwrap();

		while sent.try_recv().is_ok() {}

		let result = agent.restore_archived_thread("agent", "opaque thread/1").await;

		match expected {
			"rejected" => assert!(matches!(result, Err(AgentError::Rejected(_)))),
			"unknown" => assert!(matches!(result, Err(AgentError::UnknownDispatch))),
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
	let (mut agent, mut sent, directory) = fixture_with_history(history).await;

	agent.start_agent("agent", "Initial").await.unwrap();
	agent.store.mark_agent_dispatch_unknown("agent".into()).await.unwrap();
	// Reopen the durable owner while retaining the fixture transport; native state
	// still owns the archive flag, and exact saved turn identity owns recovery.
	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();

	agent.store = SqliteStore::open(&root.paths()).unwrap();

	while sent.try_recv().is_ok() {}

	agent.restore_archived_thread("agent", "opaque thread/1").await.unwrap();

	let work = agent.store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Idle);
	assert!(work.active_turn_id.is_none());

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start");
		assert_ne!(request["method"], "thread/start");
	}
}
