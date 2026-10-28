use crate::{
	SqliteStore,
	agent::{EnqueueAgentEvent, tests},
};

#[tokio::test]
async fn exact_terminal_turn_expires_questions_without_replaying_or_answering_them() {
	for status in ["completed", "interrupted", "failed"] {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("questions.sqlite3")).unwrap();
		store.create_agent_work_item(tests::item("agent", None)).await.unwrap();
		store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();
		store.begin_agent_dispatch("agent".into()).await.unwrap();
		store.acknowledge_agent_dispatch("agent".into(), "turn".into()).await.unwrap();
		for (turn, id, live) in [
			("turn", "q1", true),
			("turn", "q2", true),
			("turn", "replay", false),
			("next", "later", true),
		] {
			let questions = vec![(
				id.into(),
				serde_json::json!({"id":id,"title":"Question","options":[]}).to_string(),
			)];
			if live {
				store
					.record_live_agent_async_questions(
						"thread".into(),
						turn.into(),
						id.into(),
						questions,
					)
					.await
					.unwrap();
			} else {
				store
					.record_agent_async_questions(
						"thread".into(),
						turn.into(),
						id.into(),
						questions,
					)
					.await
					.unwrap();
			}
		}
		assert!(store.expired_agent_async_question_ids("agent".into()).await.unwrap().is_empty());
		store.complete_agent_turn_with_event("agent".into(), "turn".into(), EnqueueAgentEvent {
            source_event_id: "turn-end".into(), work_item_id: "agent".into(), event_kind: "agent_turn_completed".into(),
            payload: serde_json::json!({"terminal":{"threadId":"thread","turn":{"id":"turn","status":status}}}).to_string(),
        }).await.unwrap();
		assert_eq!(
			store
				.read_agent_async_questions("agent".into())
				.await
				.unwrap()
				.iter()
				.map(|q| q.question_id.as_str())
				.collect::<Vec<_>>(),
			["later"]
		);
		assert_eq!(
			store.expired_agent_async_question_ids("agent".into()).await.unwrap(),
			["q1", "q2"]
		);
		store.resolve_agent_async_questions("thread".into(), vec!["q1".into()]).await.unwrap();
		assert_eq!(store.expired_agent_async_question_ids("agent".into()).await.unwrap(), ["q2"]);
		store.create_agent_work_item(tests::item("other", None)).await.unwrap();
		store.bind_agent_thread("other".into(), "other-thread".into()).await.unwrap();
		assert!(store.expired_agent_async_question_ids("other".into()).await.unwrap().is_empty());
	}
}
