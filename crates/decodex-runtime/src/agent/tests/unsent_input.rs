use std::iter;

use crate::agent::tests::*;
use decodex_core::DecodexRoot;

#[tokio::test]
async fn only_local_refusals_without_prior_effects_release_input_after_restart() {
	for (error, no_prior_effects, unsent) in [
		(ClientError::RequestTooLarge, true, true),
		(ClientError::RequestQueueFull, true, true),
		(ClientError::RequestTooLarge, false, false),
		(ClientError::RequestQueueFull, false, false),
		(ClientError::FrameTooLarge, true, false),
		(ClientError::CapacityExceeded, true, false),
		(ClientError::Closed, true, false),
		(ClientError::Io, true, false),
	] {
		let (mut agent, mut sent, directory) = fixture().await;

		agent.start_agent("agent", "Start").await.unwrap();

		complete(&mut agent, "agent").await;

		let previous = agent.store.get_agent_work_item("agent".into()).await.unwrap();

		agent.enqueue_user_message("agent", "pending", "Keep this exact input").await.unwrap();

		let event = agent
			.store
			.list_pending_agent_events(100)
			.await
			.unwrap()
			.into_iter()
			.find(|e| e.event_kind == "user_message")
			.unwrap();

		agent
			.store
			.begin_agent_dispatch_with_input("agent".into(), vec![event.id], None)
			.await
			.unwrap();

		while sent.try_recv().is_ok() {}

		assert!(
			agent
				.finish_dispatch_attempt(
					&previous,
					None,
					Err(error.into()),
					None,
					no_prior_effects,
					None
				)
				.await
				.is_err()
		);

		let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
		let reopened = SqliteStore::open(&root.paths()).unwrap();
		let saved = reopened.get_agent_inbox_event(event.id).await.unwrap();

		assert!(saved.payload.contains("Keep this exact input"));

		let work = reopened.get_agent_work_item("agent".into()).await.unwrap();

		if unsent {
			assert_eq!(saved.disposition, Some(AgentDisposition::UserDecision));
			assert!(saved.delivered_turn_id.is_none());
			assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Idle);
			assert_eq!(work.status, AgentWorkStatus::UserDecision);

			agent.check_due_followups(i64::MAX).await.unwrap();
		} else {
			assert!(saved.disposition.is_none());
			assert_eq!(saved.delivered_turn_id.as_deref(), Some(""));
			assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Unknown);
		}

		assert!(
			iter::from_fn(|| sent.try_recv().ok()).all(|r| r["method"] != "turn/start"),
			"a refusal is not retry authority"
		);
	}
}

#[tokio::test]
async fn local_question_refusal_releases_only_the_exact_pending_answer() {
	let (mut agent, _sent, _directory) = fixture().await;

	agent.start_agent("agent", "Start").await.unwrap();

	complete(&mut agent, "agent").await;

	let previous = agent.store.get_agent_work_item("agent".into()).await.unwrap();
	let event = agent
		.store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "reply".into(),
			work_item_id: "agent".into(),
			event_kind: "async_question_answer".into(),
			payload: serde_json::json!({"text":"My answer","asyncQuestionId":"question"})
				.to_string(),
		})
		.await
		.unwrap();

	agent
		.store
		.begin_agent_dispatch_with_input("agent".into(), vec![event.id], None)
		.await
		.unwrap();

	assert!(
		agent
			.finish_dispatch_attempt(
				&previous,
				Some(event.id),
				Err(ClientError::RequestQueueFull.into()),
				None,
				true,
				None
			)
			.await
			.is_err()
	);

	let saved = agent.store.get_agent_inbox_event(event.id).await.unwrap();

	assert_eq!(saved.disposition, Some(AgentDisposition::Resolved));
	assert!(saved.disposition_note.unwrap().contains("local connection queue"));
	assert!(
		!agent.store.agent_async_answer_pending("agent".into(), "question".into()).await.unwrap()
	);
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Idle
	);
}
