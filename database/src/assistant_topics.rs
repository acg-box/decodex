//! Personal topic membership is recorded atomically with its local conversation.
use rusqlite::{Connection, OptionalExtension as _, Transaction};
use serde_json::json;

use crate::{SqliteStore, StoreError, error};

pub(crate) fn owner(c: &Connection, conversation: &str) -> Result<Option<String>, StoreError> {
	c.query_row("SELECT a.id FROM agents a WHERE a.assistant=1 AND (a.id=?1 OR EXISTS(SELECT 1 FROM agent_inbox_events e JOIN agents t ON t.id=e.work_item_id WHERE e.work_item_id=?1 AND e.event_kind='assistant_internal_topic' AND json_extract(e.payload,'$.assistantId')=a.id))",[conversation],|r|r.get(0)).optional().map_err(|e|error::sqlite_error(e).into())
}

pub(super) fn ids(
	c: &Connection,
	assistant: &str,
	retired: Option<bool>,
) -> Result<Vec<String>, StoreError> {
	c.prepare("SELECT e.work_item_id FROM agent_inbox_events e JOIN agents a ON a.id=e.work_item_id WHERE e.event_kind='assistant_internal_topic' AND json_extract(e.payload,'$.assistantId')=?1 AND (?2 IS NULL OR coalesce(json_extract(e.payload,'$.retired'),0)=?2) ORDER BY e.id DESC").map_err(error::sqlite_error)?.query_map(rusqlite::params![assistant,retired],|r|r.get(0)).map_err(error::sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(|e|error::sqlite_error(e).into())
}

pub(super) fn refresh(
	tx: &Transaction<'_>,
	assistant: &str,
	receipt: &str,
) -> Result<(), StoreError> {
	for topic in std::iter::once(assistant.to_owned()).chain(ids(tx, assistant, None)?) {
		super::record(
			tx,
			&topic,
			&format!("{receipt}:refresh:{topic}"),
			"assistant_internal_refresh",
			json!({"id":"refresh","value":true}),
		)?;
	}
	Ok(())
}

pub(crate) fn register(
	tx: &Transaction<'_>,
	conversation: &str,
	assistant: &str,
) -> Result<(), StoreError> {
	if owner(tx, assistant)?.as_deref() != Some(assistant) {
		return Err(super::invalid());
	}
	super::record(
		tx,
		conversation,
		&json!(["assistant_internal_topic", conversation]).to_string(),
		"assistant_internal_topic",
		json!({"assistantId":assistant}),
	)?;
	if super::settings(tx, assistant)?.ragflow.is_some_and(|r| r.ingest_chats) {
		super::record(
			tx,
			conversation,
			&json!(["assistant_topic_export", conversation]).to_string(),
			"assistant_internal_export_boundary",
			json!({"enabled":true}),
		)?;
	}
	Ok(())
}

pub(crate) fn retain_deleted_identity(
	tx: &Transaction<'_>,
	conversation: &str,
	assistant: &str,
) -> Result<(), StoreError> {
	super::record(
		tx,
		conversation,
		&json!(["assistant_topic_retained_owner", conversation]).to_string(),
		"assistant_internal_topic",
		json!({"assistantId":assistant,"retired":true}),
	)
}

impl SqliteStore {
	/// Resolve personal identity only through the canonical role or recorded topic membership.
	pub async fn assistant_owner(
		&self,
		conversation: String,
	) -> Result<Option<String>, StoreError> {
		self.run(move |c| owner(c, &conversation)).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{AgentDispatchState, AgentWorkItem, AgentWorkKind, AgentWorkStatus};
	use decodex_core::assistant::{AssistantCommand, AssistantSettings};

	fn conversation(id: &str) -> AgentWorkItem {
		AgentWorkItem {
			id: id.into(),
			parent_goal_id: None,
			kind: AgentWorkKind::Goal,
			title: id.into(),
			instructions: "Help the user".into(),
			codex_thread_id: None,
			status: AgentWorkStatus::Wait,
			dispatch_state: AgentDispatchState::Idle,
			active_turn_id: None,
			next_check_at_micros: None,
			created_at_micros: 1,
			updated_at_micros: 1,
		}
	}

	#[tokio::test]
	async fn personal_topics_share_one_identity_and_settings_without_enrolling_project_agents() {
		let dir = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&dir.path().join("topics.sqlite3")).unwrap();
		store.create_assistant(conversation("personal")).await.unwrap();
		store.create_agent_manager(conversation("project")).await.unwrap();
		for id in ["topic-a", "topic-b"] {
			let first =
				store.create_assistant_topic(conversation(id), "personal".into()).await.unwrap();
			assert_eq!(
				store.create_assistant_topic(conversation(id), "personal".into()).await.unwrap(),
				first
			);
			assert!(first.codex_thread_id.is_none());
			assert!(store.list_pending_agent_events(20).await.unwrap().is_empty());
			assert_eq!(
				store.assistant_owner(id.into()).await.unwrap().as_deref(),
				Some("personal")
			);
		}
		let settings =
			AssistantSettings { mandate: "Use concise work updates".into(), ..Default::default() };
		store
			.apply_assistant_command(
				"topic-a".into(),
				"settings".into(),
				AssistantCommand::Configure { settings: settings.clone() },
				true,
			)
			.await
			.unwrap();
		for id in ["personal", "topic-a", "topic-b"] {
			assert_eq!(store.assistant_settings(id.into()).await.unwrap(), Some(settings.clone()));
		}
		assert!(store.assistant_settings("project".into()).await.unwrap().is_none());
		assert!(
			store.create_assistant_topic(conversation("invalid"), "project".into()).await.is_err()
		);
		assert!(
			store.create_assistant_topic(conversation("nested"), "topic-a".into()).await.is_err()
		);
		assert!(store.get_agent_work_item("invalid".into()).await.is_err());
		let state = store.assistant_state().await.unwrap();
		assert!(store.set_assistant("topic-a".into(), true).await.is_err());
		assert_eq!(state.agent_id.as_deref(), Some("personal"));
		assert_eq!(state.topic_ids.len(), 2);
		assert_eq!(store.agents().await.unwrap().iter().filter(|agent| agent.assistant).count(), 1);
		store.discard_empty_agent_work("topic-b".into()).await.unwrap();
		assert_eq!(store.assistant_state().await.unwrap().topic_ids, vec!["topic-a"]);
		assert_eq!(store.assistant_settings("personal".into()).await.unwrap(), Some(settings));
	}

	#[tokio::test]
	async fn topic_authority_stays_with_the_original_delivered_user_event() {
		let dir = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&dir.path().join("authority.sqlite3")).unwrap();
		store.create_assistant(conversation("personal")).await.unwrap();
		for id in ["topic-a", "topic-b"] {
			store.create_assistant_topic(conversation(id), "personal".into()).await.unwrap();
		}
		store.begin_agent_thread_creation("topic-a".into()).await.unwrap();
		store.acknowledge_agent_thread_creation("topic-a".into(), "thread-a".into()).await.unwrap();
		let event = store
			.enqueue_agent_event(crate::EnqueueAgentEvent {
				source_event_id: "instruction".into(),
				work_item_id: "topic-a".into(),
				event_kind: "user_message".into(),
				payload: json!({"source":"user","text":"Keep updates concise"}).to_string(),
			})
			.await
			.unwrap()
			.id;
		store.begin_agent_dispatch_with_events("topic-a".into(), vec![event]).await.unwrap();
		store.acknowledge_agent_dispatch("topic-a".into(), "turn-a".into()).await.unwrap();

		let command = AssistantCommand::SetMandate {
			text: "Keep updates concise".into(),
			authority_event_id: event,
		};
		assert!(
			store
				.apply_assistant_command(
					"topic-b".into(),
					"wrong-source".into(),
					command.clone(),
					false
				)
				.await
				.is_err()
		);
		store
			.apply_assistant_command("topic-a".into(), "original-source".into(), command, false)
			.await
			.unwrap();
		assert_eq!(store.assistant_state().await.unwrap().settings.mandate, "Keep updates concise");
		store.set_assistant("personal".into(), false).await.unwrap();
		assert!(store.assistant_settings("topic-a".into()).await.unwrap().is_none());
		assert!(store.assistant_owner("topic-b".into()).await.unwrap().is_none());
	}

	#[tokio::test]
	async fn deleting_a_topic_with_work_references_keeps_only_its_personal_identity() {
		use decodex_core::work::{Work, WorkCommand, WorkRequirements, WorkState};
		let dir = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&dir.path().join("delete.sqlite3")).unwrap();
		store.create_assistant(conversation("personal")).await.unwrap();
		let mut topic = conversation("topic");
		topic.title = "private-topic-title".into();
		topic.instructions = "private-topic-content".into();
		store.create_assistant_topic(topic, "personal".into()).await.unwrap();
		store.begin_agent_thread_creation("topic".into()).await.unwrap();
		store
			.acknowledge_agent_thread_creation("topic".into(), "native-topic".into())
			.await
			.unwrap();
		let work = Work {
			id: "outcome".into(),
			project_id: None,
			owner_agent_id: "topic".into(),
			title: "Independent outcome".into(),
			requirements: WorkRequirements {
				id: "req".into(),
				goal: "Keep the independent Work".into(),
				criteria: "Verify its result".into(),
				source: "explicit-user-action".into(),
			},
			state: WorkState::Open,
			reason: None,
			resume_condition: None,
			user_hold: false,
			assessment: None,
		};
		store
			.apply_work_command(
				"topic".into(),
				"create-work".into(),
				WorkCommand::Create { work },
				true,
			)
			.await
			.unwrap();
		store.request_agent_thread_deletion("topic".into(), "native-topic".into()).await.unwrap();
		store.confirm_agent_thread_deletion("native-topic".into()).await.unwrap();
		assert!(store.assistant_state().await.unwrap().topic_ids.is_empty());
		assert_eq!(store.assistant_state().await.unwrap().retired_topic_ids, vec!["topic"]);
		assert_eq!(
			store.assistant_owner("topic".into()).await.unwrap().as_deref(),
			Some("personal")
		);
		assert!(
			store.agents().await.unwrap().iter().all(|agent| agent.id != "topic"),
			"a deleted personal topic cannot become a project Agent"
		);
		assert_eq!(
			store
				.agent_work_portfolio("topic".into(), Some("outcome".into()))
				.await
				.unwrap()
				.works
				.len(),
			1
		);
		let item = store.get_agent_work_item("topic".into()).await.unwrap();
		assert_eq!(item.title, "Deleted conversation");
		assert_eq!(item.instructions, "Deleted conversation");
		store
			.run(|c| {
				let payload: String = c
					.query_row(
						"SELECT group_concat(payload) FROM agent_inbox_events WHERE work_item_id='topic'",
						[],
						|r| r.get(0),
					)
					.unwrap();
				assert_eq!(
					serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
					json!({"assistantId":"personal","retired":true})
				);
				Ok(())
			})
			.await
			.unwrap();
	}
}
