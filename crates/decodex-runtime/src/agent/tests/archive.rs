use std::iter;

use crate::agent::tests::{
	self, AgentCoordinator, AgentError, ClientError, EnqueueAgentEvent, ServerEvent, SqliteStore,
};
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
async fn archived_owners_and_their_old_writer_notices_do_not_block_new_conversations() {
	for writer_notice in [false, true] {
		let (mut agent, mut sent, _directory) = tests::fixture_with_history(serde_json::json!({
			"_resume_failures": 1,
			"_resume_error": {"code":-32_600,"message":"session opaque thread/1 is archived. Run `codex unarchive opaque thread/1` to unarchive it first."}
		})).await;

		agent.start_agent("agent", "Initial").await.unwrap();
		agent.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: serde_json::json!({"threadId":"opaque thread/1","turn":{"id":"opaque turn/1","status":"completed","items":[]}}),
		}).await.unwrap();
		agent.loaded_threads.clear();

		let old_input = agent
			.store
			.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id: "old-input".into(),
				work_item_id: "agent".into(),
				event_kind: "user_message".into(),
				payload: serde_json::json!({"text":"Keep this archived draft"}).to_string(),
			})
			.await
			.unwrap();

		if writer_notice {
			agent
				.store
				.record_agent_thread_in_use("agent".into(), "External writer".into())
				.await
				.unwrap();
		}

		AgentCoordinator::create_conversation(
			&agent.store,
			"new",
			EnqueueAgentEvent {
				source_event_id: "new-input".into(),
				work_item_id: "new".into(),
				event_kind: "user_message".into(),
				payload: serde_json::json!({"text":"Start this new conversation"}).to_string(),
			},
			None,
		)
		.await
		.unwrap();

		while sent.try_recv().is_ok() {}

		agent.wake_pending().await.unwrap();

		let new = agent.store.get_agent_work_item("new".into()).await.unwrap();

		assert_eq!(new.codex_thread_id.as_deref(), Some("opaque thread/2"));
		assert_eq!(new.dispatch_state, decodex_database::AgentDispatchState::Running);

		assert!(
			agent.store.get_agent_inbox_event(old_input.id).await.is_err(),
			"archiving discards input that was never dispatched"
		);
		assert!(agent.store.list_agent_thread_in_use_events().await.unwrap().is_empty());

		let requests = iter::from_fn(|| sent.try_recv().ok()).collect::<Vec<_>>();

		assert!(!requests.iter().any(|r| r["method"] == "thread/unarchive"));

		let starts = requests.iter().filter(|r| r["method"] == "turn/start").collect::<Vec<_>>();

		assert_eq!(starts.len(), 1);
		assert_eq!(starts[0]["params"]["threadId"], "opaque thread/2");
	}
}

#[tokio::test]
async fn restore_preserves_history_identity_without_reviving_pending_input_or_creating_a_turn() {
	let (mut agent, mut sent, _directory) =
		tests::fixture_with_history(serde_json::json!({"_archived":true})).await;

	agent.start_agent("agent", "Initial").await.unwrap();
	agent.handle_event(ServerEvent::Notification {method:"turn/completed".into(),params:serde_json::json!({"threadId":"opaque thread/1","turn":{"id":"opaque turn/1","status":"completed","items":[]}})}).await.unwrap();
	agent
		.store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "queued".into(),
			work_item_id: "agent".into(),
			event_kind: "user_message".into(),
			payload: serde_json::json!({"text":"Keep this input"}).to_string(),
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

	assert!(!pending.iter().any(|event| event.source_event_id == "queued"));
}

#[tokio::test]
async fn restore_distinguishes_rejection_disconnect_and_another_clients_success() {
	for (settings, expected) in [
		(serde_json::json!({"_archived":true,"_archive_reject":true}), "rejected"),
		(serde_json::json!({"_archived":true,"_archive_disconnect":true}), "unknown"),
		(
			serde_json::json!({"_archived":true,"_archive_reject":true,"_archive_peer_restored":true}),
			"active",
		),
	] {
		let (mut agent, mut sent, _directory) = tests::fixture_with_history(settings).await;

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
	let history = serde_json::json!({"_archived":true,"opaque thread/1":{"thread":{"id":"opaque thread/1","status":{"type":"notLoaded"},"turns":[{"id":"opaque turn/1","status":"interrupted","items":[]}]}}});
	let (mut agent, mut sent, directory) = tests::fixture_with_history(history).await;

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
