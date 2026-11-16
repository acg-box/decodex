//! Empty voice reservations are not saved conversation history.
use rusqlite::{Connection, OptionalExtension as _};

use crate::{AgentWorkItem, SqliteStore, StoreError, error};

const CONTENT: &str = "SELECT EXISTS(
 SELECT 1 FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind NOT IN (
 'assistant_internal_topic','assistant_internal_export_boundary','assistant_internal_refresh',
 'assistant_internal_memory_health','assistant_internal_bootstrap','voice_conversation_draft',
 'native_task_models','native_task_permissions','native_task_plugins','native_warning','conversation_title',
 'configuration_needs_attention','thread_delete_requested','voice_overview','voice_turn')
 UNION ALL SELECT 1 FROM agent_work_items WHERE parent_goal_id=?1
 UNION ALL SELECT 1 FROM agent_dependencies WHERE work_item_id=?1 OR depends_on_id=?1
 UNION ALL SELECT 1 FROM agent_thread_revisions WHERE work_id=?1
 UNION ALL SELECT 1 FROM agent_prompt_inputs WHERE work_item_id=?1
 UNION ALL SELECT 1 FROM agent_prompt_input_chunks WHERE work_item_id=?1
 UNION ALL SELECT 1 FROM works WHERE owner_agent_id=?1
)";

fn is_unpublished(c: &Connection, work: &AgentWorkItem) -> Result<bool, StoreError> {
	if work.active_turn_id.is_some() || work.next_check_at_micros.is_some() {
		return Ok(false);
	}
	let reserved: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind IN ('assistant_internal_topic','voice_conversation_draft'))", [&work.id], |r| r.get(0)).map_err(error::sqlite_error)?;
	if !reserved {
		return Ok(false);
	}
	Ok(!c.query_row(CONTENT, [&work.id], |r| r.get::<_, bool>(0)).map_err(error::sqlite_error)?)
}

pub(super) fn is_empty(c: &Connection, work: &AgentWorkItem) -> Result<bool, StoreError> {
	if work.dispatch_state != super::AgentDispatchState::Idle {
		return Ok(false);
	}
	let active: bool = c
		.query_row(
			"SELECT EXISTS(SELECT 1 FROM agent_voice_calls WHERE work_id=?1 AND closed_at_micros IS NULL)",
			[&work.id],
			|r| r.get(0),
		)
		.map_err(error::sqlite_error)?;
	Ok(!active && is_unpublished(c, work)?)
}

impl SqliteStore {
	/// Publish received native history without enqueueing it as new user input.
	pub async fn publish_agent_native_conversation(
		&self,
		work: String,
		thread: String,
		turn: String,
	) -> Result<(), StoreError> {
		self.run(move |c| {
			let tx = c.transaction().map_err(error::sqlite_error)?;
			let bound: bool = tx
				.query_row(
					"SELECT EXISTS(SELECT 1 FROM agent_work_items WHERE id=?1 AND codex_thread_id=?2)",
					rusqlite::params![work, thread],
					|r| r.get(0),
				)
				.map_err(error::sqlite_error)?;
			if !bound {
				return Err(StoreError::InvalidInput("native content binding changed"));
			}
			crate::assistant::record(
				&tx,
				&work,
				&serde_json::json!(["native_conversation_content", thread, turn]).to_string(),
				"native_conversation_content",
				serde_json::json!({"threadId":thread,"turnId":turn}),
			)?;
			tx.commit().map_err(error::sqlite_error)?;
			Ok(())
		})
		.await
	}

	/// Internal voice reservations become public conversations only after received content.
	pub async fn unpublished_agent_conversation_drafts(&self) -> Result<Vec<String>, StoreError> {
		self.run(|c| {
			let work = c
				.prepare("SELECT * FROM agent_work_items")
				.map_err(error::sqlite_error)?
				.query_map([], super::work_row)
				.map_err(error::sqlite_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(error::sqlite_error)?;
			work.into_iter()
				.filter_map(|work| match is_unpublished(c, &work) {
					Ok(true) => Some(Ok(work.id)),
					Ok(false) => None,
					Err(error) => Some(Err(error)),
				})
				.collect()
		})
		.await
	}

	/// Read unused personal or voice drafts, excluding content and active media or work.
	pub async fn empty_agent_conversation_drafts(&self) -> Result<Vec<AgentWorkItem>, StoreError> {
		self.run(|c| {
			let work = c
				.prepare("SELECT * FROM agent_work_items")
				.map_err(error::sqlite_error)?
				.query_map([], super::work_row)
				.map_err(error::sqlite_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(error::sqlite_error)?;
			work.into_iter()
				.filter_map(|work| match is_empty(c, &work) {
					Ok(true) => Some(Ok(work)),
					Ok(false) => None,
					Err(error) => Some(Err(error)),
				})
				.collect()
		})
		.await
	}

	/// Recheck one exact draft before native cleanup can remove it.
	pub async fn agent_conversation_draft_is_empty(&self, id: String) -> Result<bool, StoreError> {
		self.run(move |c| {
			let work = c
				.query_row("SELECT * FROM agent_work_items WHERE id=?1", [&id], super::work_row)
				.optional()
				.map_err(error::sqlite_error)?;
			work.as_ref().map_or(Ok(false), |work| is_empty(c, work))
		})
		.await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{AgentDispatchState, AgentWorkKind, AgentWorkStatus, EnqueueAgentEvent};

	fn work(id: &str) -> AgentWorkItem {
		AgentWorkItem {
			id: id.into(),
			parent_goal_id: None,
			kind: AgentWorkKind::Goal,
			title: "New conversation".into(),
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
	async fn native_creation_fence_does_not_publish_or_allow_cleanup_of_a_voice_draft() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("creation.sqlite3")).unwrap();
		store.create_assistant(work("assistant")).await.unwrap();
		store
			.reserve_agent_voice_draft(work("draft"), None, Some("assistant".into()))
			.await
			.unwrap();
		store.begin_agent_thread_creation("draft".into()).await.unwrap();
		for uncertain in [false, true] {
			if uncertain {
				store.mark_agent_dispatch_unknown("draft".into()).await.unwrap();
			}
			assert_eq!(store.unpublished_agent_conversation_drafts().await.unwrap(), ["draft"]);
			assert!(store.empty_agent_conversation_drafts().await.unwrap().is_empty());
		}
		store
			.record_agent_observation(EnqueueAgentEvent {
				source_event_id: "received-native-content".into(),
				work_item_id: "draft".into(),
				event_kind: "assistant_message".into(),
				payload: "{\"text\":\"Received speech\"}".into(),
			})
			.await
			.unwrap();
		assert!(store.unpublished_agent_conversation_drafts().await.unwrap().is_empty());
		assert!(store.empty_agent_conversation_drafts().await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn first_submitted_content_atomically_publishes_a_personal_conversation() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("draft.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store.create_assistant(work("assistant")).await.unwrap();
		let input = EnqueueAgentEvent {
			source_event_id: "first".into(),
			work_item_id: "topic".into(),
			event_kind: "user_message".into(),
			payload: "{\"text\":\"First content\"}".into(),
		};
		store
			.create_assistant_conversation(work("topic"), input, "assistant".into())
			.await
			.unwrap();
		drop(store);
		let reopened = SqliteStore::open_test(&path).unwrap();
		assert_eq!(
			reopened.assistant_owner("topic".into()).await.unwrap().as_deref(),
			Some("assistant")
		);
		assert!(reopened.unpublished_agent_conversation_drafts().await.unwrap().is_empty());
		assert_eq!(reopened.list_pending_agent_events(10).await.unwrap().len(), 1);
	}

	#[tokio::test]
	async fn voice_reservations_stay_unpublished_until_received_content_and_protect_real_agents() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("voice.sqlite3")).unwrap();
		store.create_assistant(work("assistant")).await.unwrap();
		store.create_agent_manager(work("project-agent")).await.unwrap();
		store
			.reserve_agent_voice_draft(work("empty"), None, Some("assistant".into()))
			.await
			.unwrap();
		store
			.reserve_agent_voice_draft(work("spoken"), None, Some("assistant".into()))
			.await
			.unwrap();
		store
			.with_connection(|c| {
				let tx = c.transaction().map_err(error::sqlite_error)?;
				crate::assistant::record(
					&tx,
					"spoken",
					"voice-text",
					"voice_user",
					serde_json::json!({"text":"Received speech"}),
				)
				.unwrap();
				tx.commit().map_err(error::sqlite_error)?;
				Ok(())
			})
			.unwrap();
		assert_eq!(store.unpublished_agent_conversation_drafts().await.unwrap(), ["empty"]);
		let (history, _) = store.read_agent_transcript("spoken".into(), None, 32).await.unwrap();
		assert_eq!(
			history.iter().map(|event| event.event_kind.as_str()).collect::<Vec<_>>(),
			["voice_user"]
		);
		assert_eq!(
			store
				.empty_agent_conversation_drafts()
				.await
				.unwrap()
				.iter()
				.map(|w| w.id.as_str())
				.collect::<Vec<_>>(),
			["empty"]
		);
		store.discard_empty_agent_work("empty".into()).await.unwrap();
		assert!(store.get_agent_work_item("empty".into()).await.is_err());
		for id in ["assistant", "project-agent", "spoken"] {
			assert!(store.discard_empty_agent_work(id.into()).await.is_err());
			assert!(store.get_agent_work_item(id.into()).await.is_ok());
		}
	}
}
