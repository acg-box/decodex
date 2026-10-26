//! Native plugin observations retain reversions and leave transcript paging unchanged.
use crate::{EnqueueAgentEvent, SqliteStore, tests};
async fn facts(store: &SqliteStore, profile: Option<&str>, digest: char) -> i64 {
	store
		.record_agent_task_plugins_publication(
			"thread".into(),
			None,
			profile.map(|profile| serde_json::json!({"disabledPluginIds":[profile]}).to_string()),
			digest.to_string().repeat(64),
		)
		.await
		.unwrap()
		.unwrap()
}

#[tokio::test]
async fn observation_revisions_preserve_reversions_without_consuming_transcript_pages() {
	let dir = tempfile::tempdir().unwrap();
	let store = tests::bound_agent_store(&dir.path().join("observations.sqlite3")).await;
	let message = store
		.record_agent_observation(EnqueueAgentEvent {
			source_event_id: "answer".into(),
			work_item_id: "work".into(),
			event_kind: "assistant_message".into(),
			payload: serde_json::json!({"text":"Visible answer"}).to_string(),
		})
		.await
		.unwrap();
	let first = facts(&store, Some("readonly"), 'a').await;

	assert_eq!(facts(&store, Some("readonly"), 'a').await, first);

	let second = facts(&store, Some("scoped"), 'b').await;
	let reverted = facts(&store, Some("readonly"), 'a').await;

	assert!(first < second && second < reverted);
	assert_eq!(
		store.agent_task_plugins("work".into(), "thread".into(), None).await.unwrap().unwrap().id,
		reverted
	);
	assert!(
		store.agent_task_plugins("foreign".into(), "thread".into(), None).await.unwrap().is_none()
	);

	facts(&store, Some("scoped"), 'b').await;

	assert_eq!(
		store.read_agent_work_events("work".into(), 1).await.unwrap(),
		vec![message.clone()]
	);
	assert_eq!(store.read_agent_transcript("work".into(), None, 1).await.unwrap().0, vec![message]);
}
