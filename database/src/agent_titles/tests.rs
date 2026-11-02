use super::{AgentConversationTitle, SqliteStore};
use crate::tests;

async fn completed(store: &SqliteStore, n: i64) -> String {
	let turn = format!("turn-{n}");
	let copy = turn.clone();
	store.run(move |c| {
		c.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,'work','user_message','{}',1,'resolved','handled',1,'work',?2)",rusqlite::params![format!("user-{n}"),copy]).unwrap();
		c.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,'work','agent_turn_completed',?2,1,'resolved','observed',1)",rusqlite::params![serde_json::json!(["turn/completed","thread",copy]).to_string(),serde_json::json!({"terminal":{"threadId":"thread","turn":{"id":copy,"status":"completed"}}}).to_string()]).unwrap();
		Ok(())
	}).await.unwrap();
	turn
}

#[tokio::test]
async fn titles_survive_restart_without_renaming_agents_waking_work_or_consuming_transcript_pages()
{
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("titles.sqlite3");
	let store = tests::bound_agent_store(&path).await;
	let original = store.get_agent_work_item("work".into()).await.unwrap().title;
	let seed = AgentConversationTitle {
		name: Some(original.clone()),
		automatic: true,
		generated: false,
		reviewed_through: 0,
	};
	assert!(
		store
			.observe_agent_title("work".into(), "thread".into(), None, seed.clone(), true)
			.await
			.unwrap()
	);
	assert!(store.run(|connection| super::titles(connection)).await.unwrap().is_empty());
	let turn = completed(&store, 1).await;
	let review = store
		.agent_title_review("work".into(), "thread".into(), turn.clone())
		.await
		.unwrap()
		.unwrap();
	assert_eq!(review.new_exchanges, 1);
	let state = AgentConversationTitle {
		name: Some("整理旁聊标题".into()),
		generated: true,
		reviewed_through: review.completed_event,
		..seed.clone()
	};
	assert!(
		store
			.observe_agent_title("work".into(), "thread".into(), None, state.clone(), false)
			.await
			.unwrap()
	);
	assert!(
		!store.observe_agent_title("work".into(), "thread".into(), None, seed, true).await.unwrap()
	);
	assert_eq!(
		store
			.agent_title_review("work".into(), "thread".into(), turn)
			.await
			.unwrap()
			.unwrap()
			.new_exchanges,
		0
	);
	for n in 2..=5 {
		completed(&store, n).await;
	}
	assert_eq!(
		store
			.agent_title_review("work".into(), "thread".into(), "turn-5".into())
			.await
			.unwrap()
			.unwrap()
			.new_exchanges,
		4
	);
	assert!(
		store
			.read_agent_work_events("work".into(), 100)
			.await
			.unwrap()
			.iter()
			.all(|e| e.event_kind != "conversation_title")
	);
	assert!(
		store
			.read_agent_transcript("work".into(), None, 100)
			.await
			.unwrap()
			.0
			.iter()
			.all(|e| e.event_kind != "conversation_title")
	);
	assert_eq!(store.get_agent_work_item("work".into()).await.unwrap().title, original);
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	assert_eq!(
		store.run(|connection| super::titles(connection)).await.unwrap().get("thread"),
		Some(&"整理旁聊标题".into())
	);
	assert!(
		!store
			.observe_agent_title("other".into(), "thread".into(), None, state, false)
			.await
			.unwrap()
	);
}
