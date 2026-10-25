use super::*;

#[tokio::test]
async fn question_history_keeps_exact_answers_and_unknown_external_dispositions_after_reopen() {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("questions.db");
	let store = SqliteStore::open_test(&path).unwrap();
	store.create_agent_work_item(item("agent", None)).await.unwrap();
	store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();
	for id in ["q1", "q2", "external"] {
		record_question(&store, "question-turn", "question-item", id).await;
	}
	let answer=store.enqueue_agent_event(EnqueueAgentEvent {source_event_id:"reply".into(),work_item_id:"agent".into(),event_kind:"async_question_answer".into(),payload:serde_json::json!({"threadId":"thread","asyncQuestionId":"q1","text":"original reply"}).to_string()}).await.unwrap();
	store.begin_agent_dispatch_with_events("agent".into(), vec![answer.id]).await.unwrap();
	store.reconcile_agent_dispatch("agent".into(), "answer-turn".into()).await.unwrap();
	store.enqueue_agent_event(EnqueueAgentEvent {source_event_id:serde_json::json!(["turn/completed","thread","answer-turn"]).to_string(),work_item_id:"agent".into(),event_kind:"agent_turn_completed".into(),payload:serde_json::json!({"terminal":{"threadId":"thread","turn":{"id":"answer-turn","status":"completed"}}}).to_string()}).await.unwrap();
	store
		.resolve_agent_async_questions("thread".into(), vec!["q1".into(), "external".into()])
		.await
		.unwrap();
	store.enqueue_agent_event(EnqueueAgentEvent {source_event_id:"wrong-thread".into(),work_item_id:"agent".into(),event_kind:"async_question_answer".into(),payload:serde_json::json!({"threadId":"other","asyncQuestionId":"q2","text":"foreign answer"}).to_string()}).await.unwrap();
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	let rows = store.read_agent_collaboration_questions(vec!["agent".into()]).await.unwrap();
	let first = rows.iter().find(|r| r.question.question_id == "q1").unwrap();
	assert!(!first.pending);
	assert_eq!(first.answer.as_ref().unwrap().id, answer.id);
	assert_eq!(first.answer_outcome.as_deref(), Some("completed"));
	let second = rows.iter().find(|r| r.question.question_id == "q2").unwrap();
	assert!(second.pending);
	assert!(second.answer.is_none(), "a sibling thread cannot answer this exact question");
	assert!(
		rows.iter().find(|r| r.question.question_id == "external").unwrap().answer.is_none(),
		"a tombstone is not a reply body or resolution"
	);
}
