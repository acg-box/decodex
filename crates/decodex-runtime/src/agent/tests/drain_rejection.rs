use crate::agent::tests::{self, AgentError, ClientError, EnqueueAgentEvent, SqliteStore};
use decodex_codex::app_server_client::RpcError;
use decodex_core::DecodexRoot;

#[tokio::test]
async fn drain_wire_refusal_distinguishes_direct_input_from_injected_updates() {
	let (mut agent, mut sent, _directory) =
		tests::fixture_with_history(serde_json::json!({"_turn_draining":true})).await;

	assert!(matches!(
		agent.start_agent("agent", "Keep the original instruction").await,
		Err(AgentError::InputNotSent(decodex_database::AgentDispatchRefusal::ServerDraining))
	));
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Idle
	);

	let mut starts = 0;

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "thread/inject_items");

		starts += usize::from(request["method"] == "turn/start");
	}

	assert_eq!(starts, 1);

	agent.wake_pending().await.unwrap();

	assert!(sent.try_recv().is_err());

	let (mut agent, mut sent, _directory) =
		tests::fixture_with_history(serde_json::json!({"_turn_draining_after_injection":true}))
			.await;

	agent.start_agent("agent", "Initial").await.unwrap();

	tests::complete(&mut agent, "agent").await;

	while sent.try_recv().is_ok() {}

	let before = agent.store.get_agent_work_item("agent".into()).await.unwrap();
	let event = agent
		.store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "external-update".into(),
			work_item_id: "agent".into(),
			event_kind: "automation_result".into(),
			payload: serde_json::json!({"text":"External evidence"}).to_string(),
		})
		.await
		.unwrap();

	assert!(agent.dispatch_with_events(&before, "Process update", vec![event.id]).await.is_err());

	let mut methods = Vec::new();

	while let Ok(request) = sent.try_recv() {
		methods.push(request["method"].as_str().unwrap().to_owned());
	}

	assert!(
		methods.iter().position(|m| m == "thread/inject_items").unwrap()
			< methods.iter().position(|m| m == "turn/start").unwrap()
	);
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Unknown
	);
}

#[tokio::test]
async fn native_rejection_preserves_input_and_never_replays_ambiguous_effects() {
	for (managed, message) in [
		(false, "Server is draining; retry after reconnecting"),
		(
			true,
			"failed to load configuration: Your organization's required model provider settings changed. Restart Codex to apply them; this request was not sent",
		),
	] {
		for (prior_effects, lost_response, expected_rejection) in
			[(false, false, true), (true, false, false), (false, true, false)]
		{
			let (mut agent, mut sent, directory) = tests::fixture().await;

			agent.start_agent("agent", "Initial").await.unwrap();

			tests::complete(&mut agent, "agent").await;

			let before = agent.store.get_agent_work_item("agent".into()).await.unwrap();
			let event = agent
				.store
				.enqueue_agent_event(EnqueueAgentEvent {
					source_event_id: "drain-input".into(),
					work_item_id: "agent".into(),
					event_kind: "user_message".into(),
					payload: serde_json::json!({"text":"Keep this input","source":"user"})
						.to_string(),
				})
				.await
				.unwrap();

			agent
				.store
				.begin_agent_dispatch_with_input("agent".into(), vec![event.id], None)
				.await
				.unwrap();

			let error = if lost_response {
				ClientError::Closed
			} else {
				ClientError::Remote(RpcError { code: -32_600, message: message.into(), data: None })
			};
			let result = agent
				.finish_dispatch_attempt(
					&before,
					None,
					Err(error.into()),
					None,
					!prior_effects,
					None,
				)
				.await;

			assert_eq!(
				if managed {
					matches!(
						result,
						Err(AgentError::InputNotSent(
							decodex_database::AgentDispatchRefusal::ManagedProviderChanged
						))
					)
				} else {
					matches!(
						result,
						Err(AgentError::InputNotSent(
							decodex_database::AgentDispatchRefusal::ServerDraining
						))
					)
				},
				expected_rejection
			);

			while sent.try_recv().is_ok() {}

			agent.wake_pending().await.unwrap();

			assert!(sent.try_recv().is_err(), "Refused or uncertain input must not be replayed");

			drop(agent);

			let root =
				DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
			let store = SqliteStore::open(&root.paths()).unwrap();
			let work = store.get_agent_work_item("agent".into()).await.unwrap();
			let saved = store.get_agent_inbox_event(event.id).await.unwrap();

			assert!(saved.payload.contains("Keep this input"));

			if expected_rejection {
				assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Idle);
				assert_eq!(work.status, decodex_database::AgentWorkStatus::UserDecision);
				assert_eq!(
					saved.disposition,
					Some(decodex_database::AgentDisposition::UserDecision)
				);
				assert!(saved.delivered_turn_id.is_none());

				let note = saved.disposition_note.unwrap();

				assert!(note.contains("Not sent"));
				assert_eq!(note.contains("managed model provider"), managed);
			} else {
				assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Unknown);
				assert!(saved.disposition.is_none());
			}
		}
	}
}
