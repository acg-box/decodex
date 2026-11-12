//! Read only the token and model metadata needed for an API price equivalent.
use rusqlite::params;

use crate::{SqliteStore, StoreError, error};

pub struct AgentCostObservation {
	pub kind: String,
	pub payload: String,
}

impl SqliteStore {
	pub async fn read_agent_cost_observations(
		&self,
		work: String,
		thread: String,
	) -> Result<(Vec<AgentCostObservation>, bool), StoreError> {
		self.run(move |connection| {
			let mut query = connection
				.prepare(
					"SELECT e.event_kind, json_object('turnId',json_extract(e.payload,'$.turnId'),
				'tokenUsage',json_extract(e.payload,'$.tokenUsage'),
				'settings',json_extract(e.payload,'$.settings'),
				'execution',json_extract(e.payload,'$.execution'))
				FROM agent_inbox_events e JOIN agent_work_items w ON w.id=e.work_item_id
				WHERE w.id=?1 AND w.codex_thread_id=?2 AND json_extract(e.payload,'$.threadId')=?2
				AND e.event_kind IN ('token_usage','native_task_models','turn_execution')
				ORDER BY e.id LIMIT 10001",
				)
				.map_err(error::sqlite_error)?;
			let mut rows = query
				.query_map(params![work, thread], |row| {
					Ok(AgentCostObservation { kind: row.get(0)?, payload: row.get(1)? })
				})
				.map_err(error::sqlite_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(error::sqlite_error)?;
			let truncated = rows.len() > 10_000;
			rows.truncate(10_000);
			Ok((rows, truncated))
		})
		.await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		AgentDispatchState, AgentWorkItem, AgentWorkKind, AgentWorkStatus, EnqueueAgentEvent,
	};

	#[tokio::test]
	async fn cost_observations_keep_order_and_exclude_text_and_other_threads() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("cost.sqlite3")).unwrap();
		store
			.create_agent_work_item(AgentWorkItem {
				id: "root".into(),
				parent_goal_id: None,
				kind: AgentWorkKind::Goal,
				title: "Agent".into(),
				instructions: "Task".into(),
				codex_thread_id: None,
				dispatch_state: AgentDispatchState::Idle,
				active_turn_id: None,
				status: AgentWorkStatus::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			})
			.await
			.unwrap();
		store.begin_agent_thread_creation("root".into()).await.unwrap();
		store.acknowledge_agent_thread_creation("root".into(), "thread".into()).await.unwrap();
		for (id, kind, thread) in [
			("a", "native_task_models", "thread"),
			("b", "token_usage", "other"),
			("c", "user_message", "thread"),
			("d", "token_usage", "thread"),
		] {
			store.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id:id.into(), work_item_id:"root".into(), event_kind:kind.into(),
				payload:serde_json::json!({"threadId":thread,"text":"private message", "tokenUsage":{"last":{"inputTokens":1}}}).to_string(),
			}).await.unwrap();
		}
		let (rows, truncated) =
			store.read_agent_cost_observations("root".into(), "thread".into()).await.unwrap();
		assert!(!truncated);
		assert_eq!(
			rows.iter().map(|r| r.kind.as_str()).collect::<Vec<_>>(),
			vec!["native_task_models", "token_usage"]
		);
		assert!(rows.iter().all(|r| !r.payload.contains("private message")));
		assert!(
			store
				.read_agent_cost_observations("root".into(), "other".into())
				.await
				.unwrap()
				.0
				.is_empty()
		);
	}
}
