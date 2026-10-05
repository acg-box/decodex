//! Durable Agent work and inbox facts. The caller owns planning and judgment.

mod capacity;
mod steer;

pub use capacity::AgentCapacityRetry;

pub(crate) use capacity::cancel_pending as cancel_pending_capacity;

use rusqlite::{Connection, Error, OptionalExtension as _, Row, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
	AgentLiveOutput, AgentTurnExecution, DatabaseError, SqliteStore, StoreError, agent_models,
	agent_output, agent_permissions, agent_plugins, agent_prompt_edit, agent_prompt_inputs,
	agent_questions, agent_request_payload, agent_task_references, agent_turn_execution, error,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkKind {
	Goal,
	Task,
}
impl AgentWorkKind {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Goal => "goal",
			Self::Task => "task",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDisposition {
	Resolved,
	FollowUp,
	Wait,
	UserDecision,
}
impl AgentDisposition {
	fn as_str(self) -> &'static str {
		match self {
			Self::Resolved => "resolved",
			Self::FollowUp => "follow_up",
			Self::Wait => "wait",
			Self::UserDecision => "user_decision",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkStatus {
	Open,
	Resolved,
	FollowUp,
	Wait,
	UserDecision,
}
impl AgentWorkStatus {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Open => "open",
			Self::Resolved => "resolved",
			Self::FollowUp => "follow_up",
			Self::Wait => "wait",
			Self::UserDecision => "user_decision",
		}
	}
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDispatchState {
	#[default]
	Idle,
	Dispatching,
	Running,
	Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AgentWorkItem {
	pub id: String,
	pub parent_goal_id: Option<String>,
	pub kind: AgentWorkKind,
	pub title: String,
	pub instructions: String,
	/// An opaque app-server identity. No UUID or filesystem interpretation is permitted.
	pub codex_thread_id: Option<String>,
	pub dispatch_state: AgentDispatchState,
	pub active_turn_id: Option<String>,
	pub status: AgentWorkStatus,
	pub next_check_at_micros: Option<i64>,
	pub created_at_micros: i64,
	pub updated_at_micros: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AgentDependency {
	pub work_item_id: String,
	pub depends_on_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct EnqueueAgentEvent {
	/// Stable identity supplied by the event source; retries must reuse this value.
	pub source_event_id: String,
	pub work_item_id: String,
	pub event_kind: String,
	pub payload: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AgentInboxEvent {
	pub id: i64,
	pub source_event_id: String,
	pub work_item_id: String,
	pub event_kind: String,
	pub payload: String,
	pub created_at_micros: i64,
	pub disposition: Option<AgentDisposition>,
	pub disposition_note: Option<String>,
	pub disposed_at_micros: Option<i64>,
	/// Empty while dispatch is claimed or unknown; the exact turn ID after acknowledgment.
	/// This delivery fence never disposes the event.
	pub delivered_turn_id: Option<String>,
}

/// One atomic bounded read for a public projection.
pub enum AgentStoreSnapshot {
	Complete {
		managers: Vec<String>,
		workspaces: Vec<(String, String, String)>,
		work_items: Vec<AgentWorkItem>,
		dependencies: Vec<AgentDependency>,
		pending_events: Vec<AgentInboxEvent>,
	},
	CapacityExceeded {
		work_items: u64,
		dependencies: u64,
		pending_events: u64,
	},
}

/// Saved usage fields for one exact native completed turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentTurnMetrics {
	/// Exact native turn identity.
	pub turn_id: String,
	/// Acknowledged whole-turn counter delta, absent when not known.
	pub usage_json: Option<String>,
	/// Latest provider usage observation retained with this completion.
	pub observation_json: Option<String>,
}

impl SqliteStore {
	/// Read only usage fields from exact completed-turn receipts, without transcript text.
	pub async fn read_agent_turn_metrics(
		&self,
		work: String,
		thread: String,
		turns: Vec<String>,
	) -> Result<Vec<AgentTurnMetrics>, StoreError> {
		bounded(&work, 512)?;
		bounded(&thread, 512)?;

		if turns.len() > 100 {
			return Err(StoreError::InvalidInput("too many turn metrics"));
		}

		for turn in &turns {
			bounded(turn, 512)?;
		}

		let turns = serde_json::to_string(&turns)
			.map_err(|_| StoreError::InvalidInput("invalid turn identities"))?;

		self.run(move |connection| {
			connection.prepare("SELECT requested.value,json_extract(e.payload,'$.usage'),json_extract(e.payload,'$.threadReadback.tokenUsage') FROM json_each(?3) requested JOIN agent_inbox_events e ON e.source_event_id=json_array('turn/completed',?2,requested.value) WHERE e.work_item_id=?1 AND e.event_kind IN ('agent_turn_completed','worker_turn_completed') AND json_extract(e.payload,'$.terminal.threadId')=?2 AND json_extract(e.payload,'$.terminal.turn.id')=requested.value")
				.map_err(error::sqlite_error)?.query_map(rusqlite::params![work,thread,turns],|row|Ok(AgentTurnMetrics {turn_id:row.get(0)?,usage_json:row.get(1)?,observation_json:row.get(2)?}))
				.map_err(error::sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(|error|error::sqlite_error(error).into())
		}).await
	}

	/// Read the latest bounded work events in chronological order.
	pub async fn read_agent_work_events(
		&self,
		work_id: String,
		limit: usize,
	) -> Result<Vec<AgentInboxEvent>, StoreError> {
		self.read_agent_work_events_before(work_id, None, limit).await
	}

	/// Read an immutable page strictly before an event identity.
	pub async fn read_agent_work_events_before(
		&self,
		work_id: String,
		before: Option<i64>,
		limit: usize,
	) -> Result<Vec<AgentInboxEvent>, StoreError> {
		let limit = page_limit(limit)?;

		self.run(move |connection| {
			read_work(connection, &work_id)?;

			connection.prepare("SELECT * FROM (SELECT * FROM agent_inbox_events WHERE work_item_id = ?1 AND event_kind NOT IN ('reasoning_voice_handoff','turn_execution','native_task_settings','model_recovery','model_recovery_result','model_recovery_observation','model_selection_reconciled','token_usage','response_usage','live_reviewer_attempt','live_reviewer_result','native_task_permissions','permission_selection','permission_selection_result','permission_selection_observation','native_task_models','model_selection','model_selection_result','model_selection_observation','native_task_plugins','plugin_selection','plugin_selection_result','plugin_selection_observation','hook_setting_attempt','hook_setting_result','hook_setting_observation','app_setting_attempt','app_setting_result','app_setting_observation','prompt_edit_attempt','prompt_edit_observation','prompt_edit_release') AND (?3 IS NULL OR id < ?3) ORDER BY id DESC LIMIT ?2) ORDER BY id")
				.map_err(error::sqlite_error)?.query_map(rusqlite::params![work_id, limit, before], event_row)
				.map_err(error::sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(|error| error::sqlite_error(error).into())
		}).await
	}

	/// Read exact-work unconfirmed inputs, independently of transcript pages and scheduler claims.
	pub async fn read_agent_unconfirmed_inputs(
		&self,
		work_id: String,
		after: Option<i64>,
		limit: usize,
	) -> Result<Vec<AgentInboxEvent>, StoreError> {
		let limit = page_limit(limit)?;

		if after.is_some_and(|id| id < 1) {
			return Err(StoreError::InvalidInput("invalid input receipt cursor"));
		}

		self.run(move |connection| {
			read_work(connection, &work_id)?;

			connection.prepare("SELECT * FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind IN ('user_message','async_question_answer','work_instruction') AND disposition IS NULL AND (delivered_turn_id IS NULL OR delivered_turn_id='') AND (?2 IS NULL OR id>?2) ORDER BY id LIMIT ?3")
				.map_err(error::sqlite_error)?.query_map(rusqlite::params![work_id,after,limit],event_row)
				.map_err(error::sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(|error|error::sqlite_error(error).into())
		}).await
	}

	/// Read final history and partial output from one database snapshot.
	pub async fn read_agent_transcript(
		&self,
		id: String,
		before: Option<i64>,
		limit: usize,
	) -> Result<(Vec<AgentInboxEvent>, Vec<AgentLiveOutput>), StoreError> {
		let limit = page_limit(limit)?;

		self.run(move |connection| {
            let tx=connection.transaction().map_err(error::sqlite_error)?;

            read_work(&tx,&id)?;

            let events=tx.prepare("SELECT * FROM (SELECT e.* FROM agent_inbox_events e WHERE work_item_id=?1 AND event_kind NOT IN ('reasoning_voice_handoff','turn_execution','native_task_settings','model_recovery','model_recovery_result','model_recovery_observation','model_selection_reconciled','token_usage','response_usage','live_reviewer_attempt','live_reviewer_result','native_task_permissions','permission_selection','permission_selection_result','permission_selection_observation','native_task_models','model_selection','model_selection_result','model_selection_observation','native_task_plugins','plugin_selection','plugin_selection_result','plugin_selection_observation','hook_setting_attempt','hook_setting_result','hook_setting_observation','app_setting_attempt','app_setting_result','app_setting_observation','prompt_edit_attempt','prompt_edit_observation','prompt_edit_release') AND (event_kind<>'activity_started' OR NOT EXISTS(SELECT 1 FROM agent_inbox_events c WHERE c.source_event_id=json_array('activity',e.work_item_id,json_extract(e.payload,'$.turn_id'),json_extract(e.payload,'$.item_id'),'completed'))) AND (event_kind<>'plan_updated' OR NOT EXISTS(SELECT 1 FROM agent_inbox_events p WHERE p.work_item_id=e.work_item_id AND p.event_kind='plan_updated' AND p.delivered_turn_id=e.delivered_turn_id AND json_extract(p.payload,'$.threadId')=json_extract(e.payload,'$.threadId') AND p.id>e.id)) AND (event_kind<>'steer_pending' OR disposition IS NULL) AND (?3 IS NULL OR id<?3) ORDER BY id DESC LIMIT ?2) ORDER BY id").map_err(error::sqlite_error)?.query_map(rusqlite::params![id,limit,before],event_row).map_err(error::sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(error::sqlite_error)?;
            let live=if before.is_none() {agent_output::read_live(&tx,&id)?} else {vec![]};

            tx.commit().map_err(error::sqlite_error)?;

            Ok((events,live))
        }).await
	}

	/// Return executable manager identities, including the original root.
	pub async fn agent_manager_ids(&self) -> Result<Vec<String>, StoreError> {
		self.run(|connection| connection.prepare("SELECT id FROM agent_work_items WHERE parent_goal_id IS NULL UNION SELECT work_id FROM agent_managers").map_err(error::sqlite_error)?
            .query_map([],|row|row.get(0)).map_err(error::sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(|error|error::sqlite_error(error).into())).await
	}

	/// Read persisted project scopes owned by managers.
	pub async fn agent_workspaces(&self) -> Result<Vec<(String, String, String)>, StoreError> {
		self.run(|connection| {
			connection
				.prepare("SELECT agent_id,name,directory FROM agent_workspaces ORDER BY agent_id")
				.map_err(error::sqlite_error)?
				.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
				.map_err(error::sqlite_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(|error| error::sqlite_error(error).into())
		})
		.await
	}

	/// Read one exact inbox record for a source-bound detail request.
	pub async fn get_agent_inbox_event(&self, id: i64) -> Result<AgentInboxEvent, StoreError> {
		self.run(move |connection| read_event(connection, id)).await
	}

	pub async fn read_agent_snapshot(
		&self,
		work_limit: usize,
		dependency_limit: usize,
		event_limit: usize,
	) -> Result<AgentStoreSnapshot, StoreError> {
		page_limit(work_limit)?;
		page_limit(dependency_limit)?;
		page_limit(event_limit)?;

		self.run(move |connection| {
			let transaction = connection.transaction().map_err(error::sqlite_error)?;
			let count = |sql| -> Result<u64, StoreError> {
				let count: i64 = transaction.query_row(sql, [], |row| row.get(0)).map_err(error::sqlite_error)?;

				u64::try_from(count).map_err(|_| DatabaseError::Corrupt.into())
			};
			let work_count = count("SELECT count(*) FROM agent_work_items")?;
			let dependency_count = count("SELECT count(*) FROM agent_dependencies")?;
			let event_count = count("SELECT count(*) FROM agent_inbox_events WHERE disposition IS NULL")?;

			if work_count > work_limit as u64 || dependency_count > dependency_limit as u64 || event_count > event_limit as u64 {
				return Ok(AgentStoreSnapshot::CapacityExceeded { work_items: work_count, dependencies: dependency_count, pending_events: event_count });
			}

			let work_items = transaction.prepare("SELECT * FROM agent_work_items ORDER BY created_at_micros, id").map_err(error::sqlite_error)?.query_map([], work_row).map_err(error::sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(error::sqlite_error)?;
			let dependencies = transaction.prepare("SELECT work_item_id, depends_on_id FROM agent_dependencies ORDER BY work_item_id, depends_on_id").map_err(error::sqlite_error)?.query_map([], |row| Ok(AgentDependency { work_item_id: row.get(0)?, depends_on_id: row.get(1)? })).map_err(error::sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(error::sqlite_error)?;
			let pending_events = transaction.prepare("SELECT * FROM agent_inbox_events WHERE disposition IS NULL ORDER BY id").map_err(error::sqlite_error)?.query_map([], event_row).map_err(error::sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(error::sqlite_error)?;
            let managers=transaction.prepare("SELECT work_id FROM agent_managers").map_err(error::sqlite_error)?.query_map([],|row|row.get(0)).map_err(error::sqlite_error)?.collect::<Result<Vec<String>,_>>().map_err(error::sqlite_error)?;
            let workspaces=transaction.prepare("SELECT agent_id,name,directory FROM agent_workspaces ORDER BY agent_id").map_err(error::sqlite_error)?.query_map([],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(error::sqlite_error)?.collect::<Result<Vec<(String,String,String)>,_>>().map_err(error::sqlite_error)?;

            transaction.commit().map_err(error::sqlite_error)?;

            Ok(AgentStoreSnapshot::Complete { managers,workspaces,work_items, dependencies, pending_events })
		}).await
	}

	pub async fn create_agent_work_item(
		&self,
		item: AgentWorkItem,
	) -> Result<AgentWorkItem, StoreError> {
		self.create_agent_work_record(item, false, None, Vec::new()).await
	}

	/// Publish new work and all its dispatch prerequisites in one transaction.
	pub async fn create_agent_work_item_with_dependencies(
		&self,
		item: AgentWorkItem,
		depends_on: Vec<String>,
	) -> Result<AgentWorkItem, StoreError> {
		self.create_agent_work_record(item, false, None, depends_on).await
	}

	/// Atomically create an executable manager and its optional workspace scope.
	pub async fn create_agent_manager(
		&self,
		item: AgentWorkItem,
		workspace: Option<(String, String)>,
	) -> Result<AgentWorkItem, StoreError> {
		if item.kind != AgentWorkKind::Goal {
			return Err(StoreError::InvalidInput("manager must be a goal"));
		}

		self.create_agent_work_record(item, true, workspace, Vec::new()).await
	}

	async fn create_agent_work_record(
		&self,
		item: AgentWorkItem,
		manager: bool,
		workspace: Option<(String, String)>,
		depends_on: Vec<String>,
	) -> Result<AgentWorkItem, StoreError> {
		bounded(&item.id, 512)?;
		bounded(&item.title, 1_024)?;
		bounded(&item.instructions, 65_536)?;

		if let Some((name, directory)) = &workspace {
			bounded(name, 256)?;
			bounded(directory, 4_096)?;
		}

		if item.status != AgentWorkStatus::Open
			|| item.codex_thread_id.is_some()
			|| item.dispatch_state != AgentDispatchState::Idle
			|| item.active_turn_id.is_some()
			|| item.created_at_micros < 0
			|| item.updated_at_micros < item.created_at_micros
			|| item.next_check_at_micros.is_some_and(|time| time < 0)
		{
			return Err(StoreError::InvalidInput(
				"new Agent work must be open and unbound with valid timestamps",
			));
		}

		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(error::sqlite_error)?;

			if let Some(parent) = &item.parent_goal_id {
				let parent = read_work(&transaction, parent)?;

				if parent.kind != AgentWorkKind::Goal {
					return Err(StoreError::InvalidInput("Agent parent must be a goal"));
				}
			}

			if work_exists(&transaction, &item.id)? {
				return Err(DatabaseError::AlreadyExists.into());
			}

			transaction
				.execute(
					"INSERT INTO agent_work_items (id, parent_goal_id, kind, title, instructions,
				codex_thread_id, status, next_check_at_micros, created_at_micros, updated_at_micros)
				VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'open', ?6, ?7, ?8)",
					rusqlite::params![
						item.id,
						item.parent_goal_id,
						item.kind.as_str(),
						item.title,
						item.instructions,
						item.next_check_at_micros,
						item.created_at_micros,
						item.updated_at_micros
					],
				)
				.map_err(error::sqlite_error)?;

			if manager || item.parent_goal_id.is_none() {
				transaction
					.execute(
						"INSERT INTO agent_tool_versions(work_id,version) VALUES(?1,3)",
						[&item.id],
					)
					.map_err(error::sqlite_error)?;
			}
			if manager {
				transaction
					.execute("INSERT INTO agent_managers(work_id) VALUES(?1)", [&item.id])
					.map_err(error::sqlite_error)?;

				if let Some((name, directory)) = workspace {
					transaction
						.execute(
							"INSERT INTO agent_workspaces(agent_id,name,directory) VALUES(?1,?2,?3)",
							rusqlite::params![item.id, name, directory],
						)
						.map_err(error::sqlite_error)?;
				}
			}

			for dependency in depends_on {
				insert_dependency(&transaction, &item.id, &dependency)?;
			}

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(item)
		})
		.await
	}

	pub async fn list_agent_work_items(&self) -> Result<Vec<AgentWorkItem>, StoreError> {
		self.run(|connection| {
			let mut statement = connection
				.prepare("SELECT * FROM agent_work_items ORDER BY created_at_micros, id")
				.map_err(error::sqlite_error)?;

			statement
				.query_map([], work_row)
				.map_err(error::sqlite_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(|error| error::sqlite_error(error).into())
		})
		.await
	}

	pub async fn get_agent_work_item(&self, id: String) -> Result<AgentWorkItem, StoreError> {
		self.run(move |connection| read_work(connection, &id)).await
	}

	pub async fn set_agent_work_status(
		&self,
		id: String,
		status: AgentWorkStatus,
		next_check_at_micros: Option<i64>,
	) -> Result<AgentWorkItem, StoreError> {
		if next_check_at_micros.is_some_and(|time| time < 0)
			|| (status == AgentWorkStatus::Resolved && next_check_at_micros.is_some())
		{
			return Err(StoreError::InvalidInput("invalid Agent next check"));
		}

		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;

			read_work(&transaction, &id)?;

			transaction.execute("UPDATE agent_work_items SET status = ?2, next_check_at_micros = ?3, updated_at_micros = max(updated_at_micros, ?4) WHERE id = ?1",
				rusqlite::params![id, status.as_str(), next_check_at_micros, crate::unix_micros()?]).map_err(error::sqlite_error)?;

			let item = read_work(&transaction, &id)?;

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(item)
		}).await
	}

	pub async fn bind_agent_thread(
		&self,
		id: String,
		codex_thread_id: String,
	) -> Result<AgentWorkItem, StoreError> {
		bounded(&codex_thread_id, 512)?;

		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let item = read_work(&transaction, &id)?;

			if let Some(bound) = &item.codex_thread_id {
				return if bound == &codex_thread_id { Ok(item) } else { Err(DatabaseError::Conflict.into()) };
			}

			let used: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM agent_work_items WHERE codex_thread_id = ?1)", [&codex_thread_id], |row| row.get(0)).map_err(error::sqlite_error)?;

			if used { return Err(DatabaseError::Conflict.into()); }

			transaction.execute("UPDATE agent_work_items SET codex_thread_id = ?2, updated_at_micros = max(updated_at_micros, ?3) WHERE id = ?1",
				rusqlite::params![id, codex_thread_id, crate::unix_micros()?]).map_err(error::sqlite_error)?;

			let updated = read_work(&transaction, &id)?;

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(updated)
		}).await
	}

	pub async fn add_agent_dependency(
		&self,
		id: String,
		depends_on: String,
	) -> Result<(), StoreError> {
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(error::sqlite_error)?;

			insert_dependency(&transaction, &id, &depends_on)?;

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(())
		})
		.await
	}

	pub async fn begin_agent_dispatch(&self, id: String) -> Result<AgentWorkItem, StoreError> {
		self.begin_agent_dispatch_with_events(id, Vec::new()).await
	}

	/// Fence the first thread creation before contacting the provider.
	pub async fn begin_agent_thread_creation(
		&self,
		id: String,
	) -> Result<AgentWorkItem, StoreError> {
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let work = read_work(&transaction, &id)?;

			if work.dispatch_state != AgentDispatchState::Idle || work.codex_thread_id.is_some() { return Err(DatabaseError::Conflict.into()); }

			transaction.execute("UPDATE agent_work_items SET dispatch_state = 'dispatching', updated_at_micros = max(updated_at_micros, ?2) WHERE id = ?1", rusqlite::params![id, crate::unix_micros()?]).map_err(error::sqlite_error)?;

			let work = read_work(&transaction, &id)?;

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(work)
		}).await
	}

	/// Bind one positively acknowledged first thread and release only its creation fence.
	pub async fn acknowledge_agent_thread_creation(
		&self,
		id: String,
		thread_id: String,
	) -> Result<AgentWorkItem, StoreError> {
		bounded(&thread_id, 512)?;

		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let work = read_work(&transaction, &id)?;

			if work.dispatch_state != AgentDispatchState::Dispatching || work.codex_thread_id.is_some() { return Err(DatabaseError::Conflict.into()); }

			transaction.execute("UPDATE agent_work_items SET codex_thread_id = ?2, dispatch_state = 'idle', updated_at_micros = max(updated_at_micros, ?3) WHERE id = ?1", rusqlite::params![id, thread_id, crate::unix_micros()?]).map_err(error::sqlite_error)?;

			let work = read_work(&transaction, &id)?;

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(work)
		}).await
	}

	/// Claim before the external effect. A restart never resets a dispatch claim.
	/// Claiming delivery does not dispose events.
	pub async fn begin_agent_dispatch_with_events(
		&self,
		id: String,
		event_ids: Vec<i64>,
	) -> Result<AgentWorkItem, StoreError> {
		self.begin_agent_dispatch_with_input(id, event_ids, None).await
	}

	/// Atomically save a manager instruction with its dispatch claim, without a wake event.
	pub async fn begin_agent_dispatch_with_input(
		&self,
		id: String,
		event_ids: Vec<i64>,
		instruction: Option<String>,
	) -> Result<AgentWorkItem, StoreError> {
		if let Some(text) = &instruction {
			bounded(text, 65_536)?;
		}

		if event_ids.len() > 1_000 {
			return Err(StoreError::InvalidInput("too many Agent dispatch events"));
		}

		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let work = read_work(&transaction, &id)?;

			if work.dispatch_state != AgentDispatchState::Idle || work.codex_thread_id.is_none() || (agent_permissions::pending(&transaction, &id)? || agent_plugins::pending(&transaction, &id)? || agent_models::pending(&transaction, &id)? || agent_prompt_edit::pending(&transaction, &id)?) {
				return Err(DatabaseError::Conflict.into());
			}

			capacity::cancel_pending(&transaction, &id)?;

			transaction.execute("UPDATE agent_work_items SET dispatch_state = 'dispatching', updated_at_micros = max(updated_at_micros, ?2) WHERE id = ?1", rusqlite::params![id, crate::unix_micros()?]).map_err(error::sqlite_error)?;

			for event_id in event_ids {
				let changed = transaction.execute("WITH RECURSIVE owned(id) AS (
					SELECT ?2 UNION SELECT child.id FROM agent_work_items child JOIN owned ON child.parent_goal_id = owned.id WHERE owned.id=?2 OR NOT EXISTS(SELECT 1 FROM agent_managers WHERE work_id=owned.id))
					UPDATE agent_inbox_events SET delivery_work_item_id = ?2, delivered_turn_id = ''
					WHERE id = ?1 AND work_item_id IN (SELECT id FROM owned) AND disposition IS NULL
					AND (event_kind IN ('worker_turn_completed', 'automation_result', 'followup_due', 'user_message') OR (event_kind='async_question_answer' AND work_item_id=?2 AND delivered_turn_id IS NULL)) AND (work_item_id=?2 OR event_kind='worker_turn_completed' OR NOT EXISTS(SELECT 1 FROM agent_managers WHERE work_id=work_item_id)) AND (work_item_id<>?2 OR event_kind<>'worker_turn_completed')
					AND (delivered_turn_id IS NULL OR (delivery_work_item_id = ?2 AND delivered_turn_id != ''))", rusqlite::params![event_id, id]).map_err(error::sqlite_error)?;

				if changed != 1 { return Err(DatabaseError::Conflict.into()); }

                let event = read_event(&transaction, event_id)?;

                if event.event_kind == "user_message" { agent_questions::retire_for_prompt(&transaction, &id, &event.payload)?; }
			}

			if work.kind == AgentWorkKind::Task || transaction.query_row("SELECT EXISTS(SELECT 1 FROM agent_managers WHERE work_id=?1)",[&id],|row|row.get::<_,bool>(0)).map_err(error::sqlite_error)? {
				transaction.execute("UPDATE agent_work_items SET status = 'open', next_check_at_micros = NULL WHERE id = ?1", [&id]).map_err(error::sqlite_error)?;
			}

			if let Some(text)=instruction {
				let now=crate::unix_micros()?;
				let previous:i64=transaction.query_row("SELECT coalesce(max(id),0) FROM agent_inbox_events WHERE work_item_id=?1",[&id],|row|row.get(0)).map_err(error::sqlite_error)?;
				let source=serde_json::json!(["work_instruction",id,previous]).to_string();
				let payload=serde_json::json!({"text":text,"source":"manager"}).to_string();

				if payload.len()>65_536 {return Err(StoreError::InvalidInput("instruction exceeds saved event bound"));}

				transaction.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,?2,'work_instruction',?3,?4,?2,'')",rusqlite::params![source,id,payload,now]).map_err(error::sqlite_error)?;
			}

			let work = read_work(&transaction, &id)?;

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(work)
		}).await
	}

	/// Fence a steering attempt before the provider effect. Pending attempts never queue.
	pub async fn begin_agent_steer(
		&self,
		id: String,
		turn: String,
		key: String,
		payload: String,
	) -> Result<i64, StoreError> {
		bounded(&payload, 65_536)?;
		bounded(&key, 512)?;

		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let work = read_work(&tx, &id)?;

			if work.dispatch_state != AgentDispatchState::Running || work.active_turn_id.as_deref() != Some(&turn) {
				return Err(DatabaseError::Conflict.into());
			}

			agent_task_references::validate_references(&tx, &payload)?;

			let mut value: Value = serde_json::from_str(&payload)
				.map_err(|_| StoreError::InvalidInput("invalid steering input"))?;
			let fields = value.as_object_mut().ok_or(StoreError::InvalidInput("invalid steering input"))?;

			fields.insert("threadId".into(), serde_json::json!(work.codex_thread_id));

			let payload = value.to_string();

			bounded(&payload, 65_536)?;

			let source = serde_json::json!(["user_steer", id, key]).to_string();

			tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,?2,'steer_pending',?3,?4,?2,?5)",rusqlite::params![source,id,payload,crate::unix_micros()?,turn]).map_err(error::sqlite_error)?;

			let event = tx.last_insert_rowid();

			tx.commit().map_err(error::sqlite_error)?;

			Ok(event)
		}).await
	}

	/// Publish only confirmed steering input as a delivered user message.
	pub async fn finish_agent_steer(&self, event: i64, accepted: bool) -> Result<(), StoreError> {
		self.run(move |connection| {
			let tx = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(error::sqlite_error)?;

			steer::finish(&tx, event, accepted)?;

			tx.commit().map_err(error::sqlite_error)?;

			Ok(())
		})
		.await
	}

	pub async fn acknowledge_agent_dispatch(
		&self,
		id: String,
		turn_id: String,
	) -> Result<AgentWorkItem, StoreError> {
		self.acknowledge_agent_dispatch_with_execution(id, turn_id, None).await
	}

	/// Bind requested execution settings in the same transaction as the native acknowledgment.
	pub async fn acknowledge_agent_dispatch_with_execution(
		&self,
		id: String,
		turn_id: String,
		execution: Option<AgentTurnExecution>,
	) -> Result<AgentWorkItem, StoreError> {
		bounded(&turn_id, 512)?;

		if let Some(execution) = &execution {
			execution.validate()?;
		}

		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let work = read_work(&transaction, &id)?;

			if work.dispatch_state != AgentDispatchState::Dispatching { return Err(DatabaseError::Conflict.into()); }

			if let Some(execution) = &execution {
				let thread = work.codex_thread_id.as_deref().ok_or(DatabaseError::Conflict)?;

				agent_turn_execution::record(&transaction, &id, thread, &turn_id, execution)?;
			}

			transaction.execute("UPDATE agent_work_items SET dispatch_state = 'running', active_turn_id = ?2, updated_at_micros = max(updated_at_micros, ?3) WHERE id = ?1", rusqlite::params![id, turn_id, crate::unix_micros()?]).map_err(error::sqlite_error)?;
			transaction.execute("UPDATE agent_usage SET turn_id=?2,baseline_input_tokens=json_extract(usage_json,'$.input_tokens'),baseline_output_tokens=json_extract(usage_json,'$.output_tokens'),turn_input_tokens=NULL,turn_output_tokens=NULL WHERE work_id=?1 AND thread_id=?3",rusqlite::params![id,turn_id,work.codex_thread_id]).map_err(error::sqlite_error)?;
			transaction.execute("UPDATE agent_inbox_events SET delivered_turn_id = ?2 WHERE delivery_work_item_id = ?1 AND delivered_turn_id = '' AND disposition IS NULL", rusqlite::params![id, turn_id]).map_err(error::sqlite_error)?;
			transaction.execute("UPDATE agent_inbox_events SET disposition='resolved',disposition_note='Instruction accepted by the provider; completion is tracked separately.',disposed_at_micros=max(created_at_micros,?3) WHERE delivery_work_item_id=?1 AND delivered_turn_id=?2 AND event_kind='work_instruction' AND disposition IS NULL",rusqlite::params![id,turn_id,crate::unix_micros()?]).map_err(error::sqlite_error)?;
			transaction.execute("UPDATE agent_capacity_retries SET state='submitted',retry_turn_id=?2 WHERE work_item_id=?1 AND state='claimed'",rusqlite::params![id,turn_id]).map_err(error::sqlite_error)?;

			let work = read_work(&transaction, &id)?;

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(work)
		}).await
	}

	pub async fn complete_agent_turn(
		&self,
		id: String,
		turn_id: String,
	) -> Result<AgentWorkItem, StoreError> {
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let work = read_work(&transaction, &id)?;

			if work.dispatch_state != AgentDispatchState::Running || work.active_turn_id.as_deref() != Some(turn_id.as_str()) {
				return Err(DatabaseError::Conflict.into());
			}

			transaction.execute("UPDATE agent_work_items SET dispatch_state = 'idle', active_turn_id = NULL, updated_at_micros = max(updated_at_micros, ?2) WHERE id = ?1", rusqlite::params![id, crate::unix_micros()?]).map_err(error::sqlite_error)?;

			let work = read_work(&transaction, &id)?;

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(work)
		}).await
	}

	/// Record the exact terminal event and release its running dispatch atomically.
	pub async fn complete_agent_turn_with_event(
		&self,
		id: String,
		turn_id: String,
		input: EnqueueAgentEvent,
	) -> Result<AgentInboxEvent, StoreError> {
		bounded(&input.source_event_id, 2_048)?;
		bounded(&input.event_kind, 128)?;

		if input.payload.len() > 65_536 || input.work_item_id != id {
			return Err(StoreError::InvalidInput("invalid Agent terminal event"));
		}

		let user_input_handled = ["agent_turn_completed", "worker_turn_completed"]
			.contains(&input.event_kind.as_str())
			&& serde_json::from_str::<Value>(&input.payload).is_ok_and(|payload| {
				payload.pointer("/terminal/turn/status").and_then(Value::as_str)
					== Some("completed")
			});

		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let work = read_work(&transaction, &id)?;

			if work.dispatch_state != AgentDispatchState::Running || work.active_turn_id.as_deref() != Some(turn_id.as_str()) {
				return Err(DatabaseError::Conflict.into());
			}

			let mut input = input;
            let mut payload: Value = serde_json::from_str(&input.payload).unwrap_or_default();
            let eligible = work.status == AgentWorkStatus::Open
                && matches!(input.event_kind.as_str(), "agent_turn_completed" | "worker_turn_completed")
                && payload.pointer("/terminal/turn/status").and_then(Value::as_str)==Some("failed")
                && payload.pointer("/terminal/turn/error/codexErrorInfo").and_then(Value::as_str)==Some("serverOverloaded")
                && payload.pointer("/threadReadback/capacityRetryEligible")==Some(&serde_json::json!(true));
            let mut retry = if eligible { capacity::next_retry(&transaction,&id,&turn_id,crate::unix_micros()?)? } else { None };

            if eligible && retry.is_none() {
                payload["capacityRetry"]=serde_json::json!({"exhausted":true,"attempt":3});

                let encoded=payload.to_string();

                if encoded.len()<=65_536 {input.payload=encoded;}
            }

            if let Some((attempt,due))=retry {
                payload["capacityRetry"]=serde_json::json!({"attempt":attempt,"dueAtMicros":due});

                let encoded=payload.to_string();

                if encoded.len()<=65_536 {
                    input.event_kind="capacity_retry".into();

                    input.payload=encoded;
                } else { retry=None; }
            }
            if let Some(thread) = work.codex_thread_id.as_deref() {
                agent_output::retain_partial_output(&transaction, &id, thread, &turn_id)?;
            }

			let previous = transaction.query_row("SELECT * FROM agent_inbox_events WHERE source_event_id = ?1", [&input.source_event_id], event_row).optional().map_err(error::sqlite_error)?;
			let event = if let Some(event) = previous {
				if event.work_item_id != input.work_item_id || event.event_kind != input.event_kind || event.payload != input.payload {
					return Err(StoreError::IdempotencyConflict);
				}

				event
			} else {
				transaction.execute("INSERT INTO agent_inbox_events (source_event_id, work_item_id, event_kind, payload, created_at_micros) VALUES (?1, ?2, ?3, ?4, ?5)",
					rusqlite::params![input.source_event_id, input.work_item_id, input.event_kind, input.payload, crate::unix_micros()?]).map_err(error::sqlite_error)?;

				read_event(&transaction, transaction.last_insert_rowid())?
			};

			transaction.execute("UPDATE agent_work_items SET dispatch_state = 'idle', active_turn_id = NULL, updated_at_micros = max(updated_at_micros, ?2) WHERE id = ?1", rusqlite::params![id, crate::unix_micros()?]).map_err(error::sqlite_error)?;

			let event = if event.event_kind == "agent_turn_completed" && event.disposition.is_none() {
				transaction.execute("UPDATE agent_inbox_events SET disposition = 'resolved', disposition_note = 'Agent turn receipt recorded; work judgment is unchanged.', disposed_at_micros = max(created_at_micros, ?2) WHERE id = ?1", rusqlite::params![event.id, crate::unix_micros()?]).map_err(error::sqlite_error)?;

				read_event(&transaction, event.id)?
			} else { event };

			if user_input_handled {
				transaction.execute("UPDATE agent_inbox_events SET disposition = 'resolved', disposition_note = 'User input handled by completed Agent turn; work judgment is unchanged.', disposed_at_micros = max(created_at_micros, ?3) WHERE disposition IS NULL AND event_kind IN ('user_message', 'async_question_answer') AND delivery_work_item_id = ?1 AND delivered_turn_id = ?2", rusqlite::params![id, turn_id, crate::unix_micros()?]).map_err(error::sqlite_error)?;
			}

            if let Some((attempt,due))=retry {
                transaction.execute("INSERT INTO agent_capacity_retries (event_id,work_item_id,failed_turn_id,attempt,due_at_micros,state) VALUES (?1,?2,?3,?4,?5,'pending')",rusqlite::params![event.id,id,turn_id,attempt,due]).map_err(error::sqlite_error)?;
            }

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(event)
		}).await
	}

	/// A caller with positive external turn evidence can reconcile an unknown dispatch.
	/// This never resets a claim to idle or authorizes a new external dispatch.
	pub async fn reconcile_agent_dispatch(
		&self,
		id: String,
		turn_id: String,
	) -> Result<AgentWorkItem, StoreError> {
		bounded(&turn_id, 512)?;

		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let work = read_work(&transaction, &id)?;

			if !matches!(work.dispatch_state, AgentDispatchState::Dispatching | AgentDispatchState::Unknown) { return Err(DatabaseError::Conflict.into()); }
			if work.active_turn_id.as_deref().is_some_and(|active| active != turn_id) {
				return Err(DatabaseError::Conflict.into());
			}

			transaction.execute("UPDATE agent_work_items SET dispatch_state = 'running', active_turn_id = ?2, updated_at_micros = max(updated_at_micros, ?3) WHERE id = ?1", rusqlite::params![id, turn_id, crate::unix_micros()?]).map_err(error::sqlite_error)?;
			transaction.execute("UPDATE agent_inbox_events SET delivered_turn_id = ?2 WHERE delivery_work_item_id = ?1 AND delivered_turn_id = '' AND disposition IS NULL", rusqlite::params![id, turn_id]).map_err(error::sqlite_error)?;
			transaction.execute("UPDATE agent_capacity_retries SET state='submitted',retry_turn_id=?2 WHERE work_item_id=?1 AND state='claimed'",rusqlite::params![id,turn_id]).map_err(error::sqlite_error)?;

			let work = read_work(&transaction, &id)?;

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(work)
		}).await
	}

	/// Preserve an ambiguous external effect and any acknowledged turn identity.
	/// This state has no automatic retry transition.
	pub async fn mark_agent_dispatch_unknown(
		&self,
		id: String,
	) -> Result<AgentWorkItem, StoreError> {
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let work = read_work(&transaction, &id)?;

			if !matches!(work.dispatch_state, AgentDispatchState::Dispatching | AgentDispatchState::Running) { return Err(DatabaseError::Conflict.into()); }

			transaction.execute("UPDATE agent_work_items SET dispatch_state = 'unknown', updated_at_micros = max(updated_at_micros, ?2) WHERE id = ?1", rusqlite::params![id, crate::unix_micros()?]).map_err(error::sqlite_error)?;

			let work = read_work(&transaction, &id)?;

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(work)
		}).await
	}

	pub async fn list_agent_dependencies(&self) -> Result<Vec<AgentDependency>, StoreError> {
		self.run(|connection| {
			let mut statement = connection.prepare("SELECT work_item_id, depends_on_id FROM agent_dependencies ORDER BY work_item_id, depends_on_id").map_err(error::sqlite_error)?;

			statement.query_map([], |row| Ok(AgentDependency { work_item_id: row.get(0)?, depends_on_id: row.get(1)? })).map_err(error::sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(|error| error::sqlite_error(error).into())
		}).await
	}

	pub async fn enqueue_agent_event(
		&self,
		input: EnqueueAgentEvent,
	) -> Result<AgentInboxEvent, StoreError> {
		self.insert_agent_event(input, false).await
	}

	/// Save a provider observation without creating a model wake or an unresolved obligation.
	pub async fn record_agent_observation(
		&self,
		input: EnqueueAgentEvent,
	) -> Result<AgentInboxEvent, StoreError> {
		if !matches!(
			input.event_kind.as_str(),
			"assistant_message" | "token_usage" | "context_compacted"
		) || !serde_json::from_str::<Value>(&input.payload).is_ok_and(|value| value.is_object())
		{
			return Err(StoreError::InvalidInput("invalid Agent observation"));
		}

		self.insert_agent_event(input, true).await
	}

	/// Read the last observed usage for one exact work turn, including after restart.
	pub async fn read_agent_usage_observation(
		&self,
		work_id: String,
		thread_id: String,
		turn_id: String,
	) -> Result<Option<AgentInboxEvent>, StoreError> {
		self.run(move |connection| {
			connection.query_row("SELECT * FROM agent_inbox_events WHERE work_item_id = ?1 AND event_kind = 'token_usage' AND json_extract(payload, '$.threadId') = ?2 AND json_extract(payload, '$.turnId') = ?3 ORDER BY id DESC LIMIT 1", rusqlite::params![work_id, thread_id, turn_id], event_row)
				.optional().map_err(|error| error::sqlite_error(error).into())
		}).await
	}

	async fn insert_agent_event(
		&self,
		input: EnqueueAgentEvent,
		observation: bool,
	) -> Result<AgentInboxEvent, StoreError> {
		bounded(&input.source_event_id, 2_048)?;
		bounded(&input.event_kind, 128)?;

		let compact = agent_request_payload::compact(&input)?;

		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let previous = transaction.query_row("SELECT * FROM agent_inbox_events WHERE source_event_id = ?1", [&input.source_event_id], event_row).optional().map_err(error::sqlite_error)?;

			if let Some(event) = previous {
				let event = agent_request_payload::hydrate(&transaction, event)?;

				return if event.work_item_id == input.work_item_id && event.event_kind == input.event_kind && event.payload == input.payload {
					Ok(event)
				} else { Err(StoreError::IdempotencyConflict) };
			}

			if !work_exists(&transaction, &input.work_item_id)? { return Err(DatabaseError::NotFound.into()); }
			if input.event_kind == "user_message" && transaction.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind='thread_in_use_needs_attention' AND disposition IS NULL)", [&input.work_item_id], |row| row.get::<_, bool>(0)).map_err(error::sqlite_error)? {
				return Err(StoreError::AgentThreadInUse);
			}
			if matches!(input.event_kind.as_str(), "user_message" | "async_question_answer" | "steer_pending") && agent_prompt_edit::pending(&transaction, &input.work_item_id)? { return Err(DatabaseError::Conflict.into()); }
			if input.event_kind == "user_message" { agent_task_references::validate_references(&transaction, &input.payload)?; agent_prompt_inputs::validate_queued_input(&transaction, &input.work_item_id, &input.payload)?; }
            if input.event_kind == "user_message" && transaction.query_row("SELECT EXISTS(SELECT 1 FROM agent_misalignment m JOIN agent_work_items w ON w.id=m.work_id AND w.codex_thread_id=m.thread_id WHERE m.work_id=?1)",[&input.work_item_id],|row|row.get::<_,bool>(0)).map_err(error::sqlite_error)? { return Err(StoreError::InvalidInput("conversation paused for provider findings")); }

			let now = crate::unix_micros()?;

			transaction.execute("INSERT INTO agent_inbox_events (source_event_id, work_item_id, event_kind, payload, created_at_micros, disposition, disposition_note, disposed_at_micros) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
				rusqlite::params![input.source_event_id, input.work_item_id, input.event_kind, compact.as_ref().unwrap_or(&input.payload), now,
					observation.then_some("resolved"), observation.then_some("Provider observation recorded; work judgment unchanged."), observation.then_some(now)]).map_err(error::sqlite_error)?;

			let event_id = transaction.last_insert_rowid();
            if input.event_kind == "user_message"
                && let Ok(payload) = serde_json::from_str::<Value>(&input.payload)
                && let Some(text) = payload["text"].as_str() {
                let title = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(64).collect::<String>();
                if !title.is_empty() {
                    transaction.execute("UPDATE agent_work_items SET title=?2,updated_at_micros=?3 WHERE id=?1 AND title='New conversation' AND codex_thread_id IS NULL", rusqlite::params![input.work_item_id,title,now]).map_err(error::sqlite_error)?;
                }
            }


			if compact.is_some() {
				transaction.execute("INSERT INTO agent_request_payloads(event_id,payload) VALUES(?1,?2)",rusqlite::params![event_id,input.payload]).map_err(error::sqlite_error)?;
			}

			let event = read_event(&transaction, event_id)?;

            if input.event_kind == "user_message" { agent_questions::retire_for_prompt(&transaction, &input.work_item_id, &input.payload)?; }

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(event)
		}).await
	}

	/// Record one active connection failure. Repeated probes do not duplicate it.
	pub async fn record_agent_connection_failure(
		&self,
		root: String,
		detail: String,
	) -> Result<(), StoreError> {
		bounded(&root, 512)?;
		bounded(&detail, 65_536)?;

		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let payload = serde_json::json!({"recovery":detail}).to_string();
            let pending: Option<(i64, String)> = tx.query_row("SELECT id,payload FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind='reconnection_needs_attention' AND disposition IS NULL ORDER BY id DESC LIMIT 1", [&root], |row| Ok((row.get(0)?,row.get(1)?))).optional().map_err(error::sqlite_error)?;

            if pending.as_ref().is_some_and(|(_, previous)| previous == &payload) { return Ok(()); }

            if let Some((id, _)) = pending {
                tx.execute("UPDATE agent_inbox_events SET disposition='resolved', disposition_note='Superseded by a newer connection diagnostic; connectivity is not yet restored.', disposed_at_micros=max(created_at_micros,?2) WHERE id=?1", rusqlite::params![id,crate::unix_micros()?]).map_err(error::sqlite_error)?;
            }

			let now = crate::unix_micros()?;
			let previous: i64 = tx.query_row("SELECT coalesce(max(id),0) FROM agent_inbox_events WHERE work_item_id=?1", [&root], |row| row.get(0)).map_err(error::sqlite_error)?;
			let source = serde_json::json!(["agent_connection", root, previous]).to_string();

			tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES(?1,?2,'reconnection_needs_attention',?3,?4)", rusqlite::params![source,root,payload,now]).map_err(error::sqlite_error)?;
			tx.commit().map_err(error::sqlite_error)?;

			Ok(())
		}).await
	}

	/// Close connection errors after an attested connection, without changing work judgment.
	pub async fn resolve_agent_connection_failure(&self, root: String) -> Result<(), StoreError> {
		self.run(move |connection| {
			connection.execute("UPDATE agent_inbox_events SET disposition='resolved',disposition_note='The Agent connection was restored.',disposed_at_micros=max(created_at_micros,?2) WHERE work_item_id=?1 AND event_kind='reconnection_needs_attention' AND disposition IS NULL",rusqlite::params![root,crate::unix_micros()?]).map_err(error::sqlite_error)?;

			Ok(())
		}).await
	}

	/// Keep one current delivery error, with separate records for later recurrences.
	pub async fn record_agent_delivery_failure(
		&self,
		root: String,
		detail: String,
	) -> Result<(), StoreError> {
		self.record_agent_delivery_notice(root, detail, "wake_failed").await
	}

	/// Record an attested external writer without treating it as an execution failure.
	pub async fn record_agent_thread_in_use(
		&self,
		root: String,
		detail: String,
	) -> Result<(), StoreError> {
		self.record_agent_delivery_notice(root, detail, "thread_in_use_needs_attention").await
	}

	async fn record_agent_delivery_notice(
		&self,
		root: String,
		detail: String,
		kind: &'static str,
	) -> Result<(), StoreError> {
		bounded(&root, 512)?;
		bounded(&detail, 2_048)?;

		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let payload = serde_json::json!({"recovery":detail}).to_string();
			let same: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind=?3 AND payload=?2 AND disposition IS NULL)",rusqlite::params![root,payload,kind],|row|row.get(0)).map_err(error::sqlite_error)?;

			if !same {
				tx.execute("UPDATE agent_inbox_events SET disposition='resolved',disposition_note='Superseded by the current delivery diagnostic.',disposed_at_micros=max(created_at_micros,?2) WHERE work_item_id=?1 AND event_kind IN ('wake_failed','followup_processing_failed','thread_in_use_needs_attention') AND disposition IS NULL",rusqlite::params![root,crate::unix_micros()?]).map_err(error::sqlite_error)?;

				let previous:i64 = tx.query_row("SELECT coalesce(max(id),0) FROM agent_inbox_events WHERE work_item_id=?1",[&root],|row|row.get(0)).map_err(error::sqlite_error)?;
				let source = serde_json::json!(["agent_delivery",root,previous]).to_string();

				tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES(?1,?2,?5,?3,?4)",rusqlite::params![source,root,payload,crate::unix_micros()?,kind]).map_err(error::sqlite_error)?;
			}

			tx.commit().map_err(error::sqlite_error)?;

			Ok(())
		}).await
	}

	/// Keep unsent input visible in history, but require a new user send after an ownership
	/// conflict.
	pub async fn hold_agent_unsent_input(&self, work: String) -> Result<(), StoreError> {
		self.run(move |connection| {
            connection.execute("UPDATE agent_inbox_events SET disposition='user_decision', disposition_note='Not sent: conversation was in use elsewhere. Send again when ready.', disposed_at_micros=max(created_at_micros,?2) WHERE work_item_id=?1 AND event_kind='user_message' AND disposition IS NULL AND delivered_turn_id IS NULL", rusqlite::params![work,crate::unix_micros()?]).map_err(error::sqlite_error)?;

            Ok(())
        }).await
	}

	/// Successful delivery processing clears only delivery diagnostics, never work events.
	pub async fn resolve_agent_delivery_failure(&self, root: String) -> Result<(), StoreError> {
		self.run(move |connection| {
			connection.execute("UPDATE agent_inbox_events SET disposition='resolved',disposition_note='Agent delivery processing recovered.',disposed_at_micros=max(created_at_micros,?2) WHERE work_item_id=?1 AND event_kind IN ('wake_failed','followup_processing_failed','thread_in_use_needs_attention') AND disposition IS NULL",rusqlite::params![root,crate::unix_micros()?]).map_err(error::sqlite_error)?;

			Ok(())
		}).await
	}

	/// Read without claiming or acknowledging. Failed processing leaves every event pending.
	pub async fn list_undelivered_agent_events(
		&self,
		limit: usize,
	) -> Result<Vec<AgentInboxEvent>, StoreError> {
		let limit = page_limit(limit)?;

		self.run(move |connection| {
			connection.prepare("SELECT * FROM agent_inbox_events WHERE disposition IS NULL AND delivered_turn_id IS NULL AND event_kind IN ('worker_turn_completed', 'automation_result', 'followup_due', 'user_message') ORDER BY id LIMIT ?1").map_err(error::sqlite_error)?.query_map([limit], event_row).map_err(error::sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(|error| error::sqlite_error(error).into())
		}).await
	}

	/// Select fresh triggers first, then unresolved evidence previously delivered to this Agent.
	/// Reading this batch does not authorize a wake without at least one fresh trigger.
	pub async fn list_agent_wake_events(
		&self,
		agent_id: String,
		limit: usize,
	) -> Result<Vec<AgentInboxEvent>, StoreError> {
		let limit = page_limit(limit)?;

		self.run(move |connection| {
			connection.prepare("WITH RECURSIVE owned(id) AS (
				SELECT ?1 UNION SELECT child.id FROM agent_work_items child JOIN owned ON child.parent_goal_id = owned.id WHERE owned.id = ?1 OR NOT EXISTS(SELECT 1 FROM agent_managers WHERE work_id=owned.id))
				SELECT * FROM agent_inbox_events WHERE work_item_id IN (SELECT id FROM owned)
				AND disposition IS NULL AND event_kind IN ('worker_turn_completed', 'automation_result', 'followup_due', 'user_message')
                AND (work_item_id <> ?1 OR event_kind <> 'worker_turn_completed') AND (work_item_id=?1 OR event_kind='worker_turn_completed' OR NOT EXISTS(SELECT 1 FROM agent_managers WHERE work_id=work_item_id))
				AND (delivered_turn_id IS NULL OR (delivery_work_item_id = ?1 AND delivered_turn_id != ''))
				ORDER BY delivered_turn_id IS NOT NULL, id LIMIT ?2")
				.map_err(error::sqlite_error)?.query_map(rusqlite::params![agent_id, limit], event_row)
				.map_err(error::sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(|error| error::sqlite_error(error).into())
		}).await
	}

	/// Read this exact turn's undisposed delivery receipts before applying the page bound.
	pub async fn list_agent_events_for_turn(
		&self,
		turn_id: String,
		limit: usize,
	) -> Result<Vec<AgentInboxEvent>, StoreError> {
		bounded(&turn_id, 512)?;

		let limit = page_limit(limit)?;

		self.run(move |connection| {
			connection.prepare("SELECT * FROM agent_inbox_events WHERE disposition IS NULL AND delivered_turn_id = ?1 ORDER BY id LIMIT ?2").map_err(error::sqlite_error)?.query_map(rusqlite::params![turn_id, limit], event_row).map_err(error::sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(|error| error::sqlite_error(error).into())
		}).await
	}

	/// Read every unresolved external-writer notice, independent of unrelated inbox pages.
	pub async fn list_agent_thread_in_use_events(
		&self,
	) -> Result<Vec<AgentInboxEvent>, StoreError> {
		self.run(|connection| {
			connection.prepare("SELECT * FROM agent_inbox_events WHERE event_kind='thread_in_use_needs_attention' AND disposition IS NULL ORDER BY id").map_err(error::sqlite_error)?
				.query_map([], event_row).map_err(error::sqlite_error)?
				.collect::<Result<Vec<_>, _>>().map_err(|error| error::sqlite_error(error).into())
		}).await
	}

	/// Read without claiming or acknowledging. Failed processing leaves every event pending.
	pub async fn list_pending_agent_events(
		&self,
		limit: usize,
	) -> Result<Vec<AgentInboxEvent>, StoreError> {
		let limit = page_limit(limit)?;

		self.run(move |connection| {
			let mut statement = connection
				.prepare(
					"SELECT * FROM agent_inbox_events WHERE disposition IS NULL ORDER BY id LIMIT ?1",
				)
				.map_err(error::sqlite_error)?;

			statement
				.query_map([limit], event_row)
				.map_err(error::sqlite_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(|error| error::sqlite_error(error).into())
		})
		.await
	}

	/// Acknowledge one response delivered to a current-connection provider request.
	/// The runtime must verify that the event is in its current connection request map.
	/// This receipt does not make a work judgment or change the next check time.
	pub async fn acknowledge_agent_request_event(
		&self,
		event_id: i64,
	) -> Result<AgentInboxEvent, StoreError> {
		self.finish_agent_request_event(
			event_id,
			"Response delivered to the current provider request; work judgment is unchanged.",
		)
		.await
	}

	/// Record the provider resolution without claiming that this client sent a response.
	pub async fn resolve_agent_request_event(
		&self,
		event_id: i64,
	) -> Result<AgentInboxEvent, StoreError> {
		self.finish_agent_request_event(event_id, "Provider resolved the current request; no local response was sent and work judgment is unchanged.").await
	}

	async fn finish_agent_request_event(
		&self,
		event_id: i64,
		note: &'static str,
	) -> Result<AgentInboxEvent, StoreError> {
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let event = read_event(&transaction, event_id)?;

			if !matches!(event.event_kind.as_str(), "permission_pending" | "user_input_pending" | "server_request_pending") {
				return Err(StoreError::InvalidInput("event is not a Agent provider request"));
			}
			if event.disposition.is_some() { return Err(DatabaseError::Conflict.into()); }

			transaction.execute("UPDATE agent_inbox_events SET disposition = 'resolved', disposition_note = ?3, disposed_at_micros = max(created_at_micros, ?2) WHERE id = ?1 AND disposition IS NULL", rusqlite::params![event_id, crate::unix_micros()?, note]).map_err(error::sqlite_error)?;

			let updated = read_event(&transaction, event_id)?;

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(updated)
		}).await
	}

	/// Resolve an idle work decision from a current Agent turn's exact user message.
	/// Original decision evidence remains immutable; the new receipt links the reply.
	pub async fn resolve_agent_user_decision(
		&self,
		agent_id: String,
		work_id: String,
		turn_id: String,
		user_event_id: i64,
		note: String,
	) -> Result<(), StoreError> {
		bounded(&note, 65_536)?;

		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let source = read_event(&transaction, user_event_id)?;
			let active: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM agent_work_items WHERE id = ?1 AND (parent_goal_id IS NULL OR EXISTS(SELECT 1 FROM agent_managers WHERE work_id=?1)) AND active_turn_id = ?2 AND dispatch_state = 'running')",rusqlite::params![agent_id,turn_id],|row|row.get(0)).map_err(error::sqlite_error)?;
			let descendant: bool = transaction.query_row("WITH RECURSIVE lineage(id,parent_goal_id) AS (SELECT id,parent_goal_id FROM agent_work_items WHERE id = ?1 UNION SELECT work.id,work.parent_goal_id FROM agent_work_items work JOIN lineage ON work.id = lineage.parent_goal_id) SELECT EXISTS(SELECT 1 FROM lineage WHERE id = ?2)",rusqlite::params![work_id,agent_id],|row|row.get(0)).map_err(error::sqlite_error)?;

			if !active || !descendant || source.work_item_id != agent_id || !matches!(source.event_kind.as_str(), "user_message" | "async_question_answer") || source.delivered_turn_id.as_deref() != Some(&turn_id) || source.disposition.is_some() {
				return Err(StoreError::InvalidInput("decision requires a current delivered user reply"));
			}

			let now = crate::unix_micros()?;
			let changed = transaction.execute("UPDATE agent_work_items SET status = 'resolved', next_check_at_micros = NULL, updated_at_micros = max(updated_at_micros, ?2) WHERE id = ?1 AND status = 'user_decision' AND dispatch_state = 'idle'",rusqlite::params![work_id,now]).map_err(error::sqlite_error)?;

			if changed != 1 {return Err(DatabaseError::Conflict.into());}

			let source_id = serde_json::json!(["user_decision_resolved",work_id,user_event_id]).to_string();
			let payload = serde_json::json!({"userEventId":user_event_id,"agentTurnId":turn_id,"summary":note}).to_string();

			transaction.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES (?1,?2,'user_decision_resolved',?3,?4,'resolved',?5,?4)",rusqlite::params![source_id,work_id,payload,now,note]).map_err(error::sqlite_error)?;
			transaction.commit().map_err(error::sqlite_error)?;

			Ok(())
		}).await
	}

	/// Record a model's explicit goal judgment with current, related evidence.
	pub async fn resolve_agent_goal(
		&self,
		agent_id: String,
		goal_id: String,
		turn_id: String,
		evidence_event_id: i64,
		note: String,
	) -> Result<(), StoreError> {
		bounded(&note, 65_536)?;

		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let event = read_event(&transaction,evidence_event_id)?;
			let current:bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM agent_work_items WHERE id = ?1 AND (parent_goal_id IS NULL OR EXISTS(SELECT 1 FROM agent_managers WHERE work_id=?1)) AND active_turn_id = ?2 AND dispatch_state = 'running')",rusqlite::params![agent_id,turn_id],|row|row.get(0)).map_err(error::sqlite_error)?;
			let related:bool = transaction.query_row("WITH RECURSIVE family(id) AS (SELECT id FROM agent_work_items WHERE id = ?1 UNION SELECT work.id FROM agent_work_items work JOIN family ON work.parent_goal_id = family.id) SELECT EXISTS(SELECT 1 FROM family WHERE id = ?2)",rusqlite::params![goal_id,event.work_item_id],|row|row.get(0)).map_err(error::sqlite_error)?;
			let user_input = matches!(event.event_kind.as_str(), "user_message" | "async_question_answer") && event.work_item_id == agent_id;
			let result = related && matches!(event.event_kind.as_str(),"worker_turn_completed"|"automation_result"|"followup_due");

			if !current || event.delivered_turn_id.as_deref() != Some(&turn_id) || (!user_input && !result) {return Err(StoreError::InvalidInput("goal resolution requires current related evidence"));}

			let now = crate::unix_micros()?;
			let changed = transaction.execute("UPDATE agent_work_items SET status = 'resolved', next_check_at_micros = NULL, updated_at_micros = max(updated_at_micros,?3) WHERE id = ?1 AND kind = 'goal' AND parent_goal_id = ?2 AND dispatch_state = 'idle' AND status <> 'resolved'",rusqlite::params![goal_id,agent_id,now]).map_err(error::sqlite_error)?;

			if changed != 1 {return Err(DatabaseError::Conflict.into());}

			let source = serde_json::json!(["goal_resolved",goal_id,evidence_event_id]).to_string();
			let payload = serde_json::json!({"evidenceEventId":evidence_event_id,"agentTurnId":turn_id,"summary":note}).to_string();

			transaction.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES (?1,?2,'goal_resolved',?3,?4,'resolved',?5,?4)",rusqlite::params![source,goal_id,payload,now,note]).map_err(error::sqlite_error)?;
			transaction.commit().map_err(error::sqlite_error)?;

			Ok(())
		}).await
	}

	/// Commit one explicit decision and the next wake time in the same transaction.
	pub async fn dispose_agent_event(
		&self,
		id: i64,
		disposition: AgentDisposition,
		note: String,
		next_check_at_micros: Option<i64>,
	) -> Result<AgentInboxEvent, StoreError> {
		bounded(&note, 65_536)?;

		if next_check_at_micros.is_some_and(|time| time < 0)
			|| (disposition == AgentDisposition::Resolved && next_check_at_micros.is_some())
		{
			return Err(StoreError::InvalidInput("invalid Agent next check"));
		}

		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let event = read_event(&transaction, id)?;

			if event.disposition.is_some() { return Err(DatabaseError::Conflict.into()); }

			let now = crate::unix_micros()?.max(event.created_at_micros);

			transaction.execute("UPDATE agent_inbox_events SET disposition = ?2, disposition_note = ?3, disposed_at_micros = ?4 WHERE id = ?1 AND disposition IS NULL",
				rusqlite::params![id, disposition.as_str(), note, now]).map_err(error::sqlite_error)?;
			transaction.execute("UPDATE agent_work_items SET status = ?2, next_check_at_micros = ?3, updated_at_micros = max(updated_at_micros, ?4) WHERE id = ?1",
				rusqlite::params![event.work_item_id, disposition.as_str(), next_check_at_micros, now]).map_err(error::sqlite_error)?;

			let updated = read_event(&transaction, id)?;

			transaction.commit().map_err(error::sqlite_error)?;

			Ok(updated)
		}).await
	}

	pub async fn list_due_agent_work_items(
		&self,
		now_micros: i64,
		limit: usize,
	) -> Result<Vec<AgentWorkItem>, StoreError> {
		let limit = page_limit(limit)?;

		self.run(move |connection| {
			let mut statement = connection.prepare("SELECT * FROM agent_work_items WHERE next_check_at_micros <= ?1 ORDER BY next_check_at_micros, id LIMIT ?2").map_err(error::sqlite_error)?;

			statement.query_map(rusqlite::params![now_micros, limit], work_row).map_err(error::sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(|error| error::sqlite_error(error).into())
		}).await
	}

	/// Select due work that has no durable notification for this exact due timestamp.
	/// The exclusion precedes the limit so old due work cannot starve later work.
	pub async fn list_unnotified_due_agent_work_items(
		&self,
		now_micros: i64,
		limit: usize,
	) -> Result<Vec<AgentWorkItem>, StoreError> {
		let limit = page_limit(limit)?;

		self.run(move |connection| {
			connection.prepare("SELECT work.* FROM agent_work_items AS work WHERE next_check_at_micros <= ?1 AND NOT EXISTS (SELECT 1 FROM agent_inbox_events AS event WHERE event.source_event_id = json_array('followup_due', work.id, work.next_check_at_micros)) ORDER BY next_check_at_micros, id LIMIT ?2").map_err(error::sqlite_error)?.query_map(rusqlite::params![now_micros, limit], work_row).map_err(error::sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(|error| error::sqlite_error(error).into())
		}).await
	}
}

pub(crate) fn read_work(connection: &Connection, id: &str) -> Result<AgentWorkItem, StoreError> {
	connection
		.query_row("SELECT * FROM agent_work_items WHERE id = ?1", [id], work_row)
		.optional()
		.map_err(error::sqlite_error)?
		.ok_or_else(|| DatabaseError::NotFound.into())
}

fn bounded(value: &str, max: usize) -> Result<(), StoreError> {
	if value.trim().is_empty() || value.len() > max {
		Err(StoreError::InvalidInput("Agent text is empty or too large"))
	} else {
		Ok(())
	}
}

fn page_limit(limit: usize) -> Result<i64, StoreError> {
	if (1..=1_000).contains(&limit) {
		Ok(limit as i64)
	} else {
		Err(StoreError::InvalidInput("Agent page size must be between 1 and 1000"))
	}
}

fn insert_dependency(
	connection: &Connection,
	id: &str,
	depends_on: &str,
) -> Result<(), StoreError> {
	if !work_exists(connection, id)? || !work_exists(connection, depends_on)? {
		return Err(DatabaseError::NotFound.into());
	}

	let cycle: bool = connection.query_row("WITH RECURSIVE ancestors(id) AS (
		SELECT ?1 UNION SELECT depends_on_id FROM agent_dependencies JOIN ancestors ON work_item_id = ancestors.id)
		SELECT EXISTS(SELECT 1 FROM ancestors WHERE id = ?2)", rusqlite::params![depends_on, id], |row| row.get(0)).map_err(error::sqlite_error)?;

	if cycle {
		return Err(StoreError::InvalidInput("Agent dependency would create a cycle"));
	}

	connection.execute("INSERT INTO agent_dependencies (work_item_id, depends_on_id) VALUES (?1, ?2) ON CONFLICT DO NOTHING", rusqlite::params![id, depends_on]).map_err(error::sqlite_error)?;

	Ok(())
}

fn work_exists(connection: &Connection, id: &str) -> Result<bool, StoreError> {
	connection
		.query_row("SELECT EXISTS(SELECT 1 FROM agent_work_items WHERE id = ?1)", [id], |row| {
			row.get(0)
		})
		.map_err(|error| error::sqlite_error(error).into())
}

fn read_event(connection: &Connection, id: i64) -> Result<AgentInboxEvent, StoreError> {
	let event = connection
		.query_row("SELECT * FROM agent_inbox_events WHERE id = ?1", [id], event_row)
		.optional()
		.map_err(error::sqlite_error)?
		.ok_or(DatabaseError::NotFound)?;

	agent_request_payload::hydrate(connection, event)
}

fn work_row(row: &Row<'_>) -> rusqlite::Result<AgentWorkItem> {
	let dispatch_state: String = row.get("dispatch_state")?;
	let dispatch_state = match dispatch_state.as_str() {
		"idle" => AgentDispatchState::Idle,
		"dispatching" => AgentDispatchState::Dispatching,
		"running" => AgentDispatchState::Running,
		"unknown" => AgentDispatchState::Unknown,
		_ => return Err(Error::InvalidQuery),
	};
	let status: String = row.get("status")?;
	let status = match status.as_str() {
		"open" => AgentWorkStatus::Open,
		"resolved" => AgentWorkStatus::Resolved,
		"follow_up" => AgentWorkStatus::FollowUp,
		"wait" => AgentWorkStatus::Wait,
		"user_decision" => AgentWorkStatus::UserDecision,
		_ => return Err(Error::InvalidQuery),
	};
	let kind: String = row.get("kind")?;
	let kind = match kind.as_str() {
		"goal" => AgentWorkKind::Goal,
		"task" => AgentWorkKind::Task,
		_ => return Err(Error::InvalidQuery),
	};

	Ok(AgentWorkItem {
		id: row.get("id")?,
		parent_goal_id: row.get("parent_goal_id")?,
		kind,
		title: row.get("title")?,
		instructions: row.get("instructions")?,
		codex_thread_id: row.get("codex_thread_id")?,
		dispatch_state,
		active_turn_id: row.get("active_turn_id")?,
		status,
		next_check_at_micros: row.get("next_check_at_micros")?,
		created_at_micros: row.get("created_at_micros")?,
		updated_at_micros: row.get("updated_at_micros")?,
	})
}

fn event_row(row: &Row<'_>) -> rusqlite::Result<AgentInboxEvent> {
	let disposition: Option<String> = row.get("disposition")?;
	let disposition = match disposition.as_deref() {
		None => None,
		Some("resolved") => Some(AgentDisposition::Resolved),
		Some("follow_up") => Some(AgentDisposition::FollowUp),
		Some("wait") => Some(AgentDisposition::Wait),
		Some("user_decision") => Some(AgentDisposition::UserDecision),
		_ => return Err(Error::InvalidQuery),
	};

	Ok(AgentInboxEvent {
		id: row.get("id")?,
		source_event_id: row.get("source_event_id")?,
		work_item_id: row.get("work_item_id")?,
		event_kind: row.get("event_kind")?,
		payload: row.get("payload")?,
		created_at_micros: row.get("created_at_micros")?,
		disposition,
		disposition_note: row.get("disposition_note")?,
		disposed_at_micros: row.get("disposed_at_micros")?,
		delivered_turn_id: row.get("delivered_turn_id")?,
	})
}

#[cfg(test)]
mod tests {
	mod activity;
	mod dispatch_refusals;
	mod inbox_carryover;
	mod native_turns;
	mod partial_output;
	mod reasoning_summary;
	mod request_payloads;
	mod steer_receipts;
	mod task_references;
	mod turn_execution;
	use std::sync::atomic::{AtomicUsize, Ordering};

	use serde_json::Value;

	use crate::{
		SqliteStore, StoreError,
		agent::{
			AgentDispatchState, AgentDisposition, AgentStoreSnapshot, AgentWorkItem, AgentWorkKind,
			AgentWorkStatus, EnqueueAgentEvent,
		},
	};

	fn capacity_failure(work: &str, turn: &str) -> EnqueueAgentEvent {
		EnqueueAgentEvent { source_event_id:format!("failure:{work}:{turn}"),work_item_id:work.into(),event_kind:"agent_turn_completed".into(),
            payload:serde_json::json!({"terminal":{"turn":{"status":"failed","error":{"codexErrorInfo":"serverOverloaded"}}},"threadReadback":{"capacityRetryEligible":true}}).to_string() }
	}

	fn item(id: &str, parent: Option<&str>) -> AgentWorkItem {
		AgentWorkItem {
			id: id.to_owned(),
			parent_goal_id: parent.map(str::to_owned),
			kind: AgentWorkKind::Goal,
			title: id.to_owned(),
			instructions: "Complete the requested work".to_owned(),
			codex_thread_id: None,
			dispatch_state: AgentDispatchState::Idle,
			active_turn_id: None,
			status: AgentWorkStatus::Open,
			next_check_at_micros: None,
			created_at_micros: 1,
			updated_at_micros: 1,
		}
	}

	async fn record_question(store: &SqliteStore, turn: &str, item_id: &str, id: &str) {
		store
			.record_agent_async_questions(
				"thread".into(),
				turn.into(),
				item_id.into(),
				vec![(
					id.to_owned(),
					serde_json::json!({"id":id,"title":"Question","options":[]}).to_string(),
				)],
			)
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn capacity_retries_are_bounded_durable_and_claimed_once() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("retry.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();

		store.create_agent_work_item(item("agent", None)).await.unwrap();
		store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();
		store.begin_agent_dispatch("agent".into()).await.unwrap();
		store.acknowledge_agent_dispatch("agent".into(), "turn-0".into()).await.unwrap();

		for attempt in 1..=3 {
			let turn = format!("turn-{}", attempt - 1);
			let event = store
				.complete_agent_turn_with_event(
					"agent".into(),
					turn.clone(),
					capacity_failure("agent", &turn),
				)
				.await
				.unwrap();

			assert_eq!(event.event_kind, "capacity_retry");
			assert!(store.list_agent_wake_events("agent".into(), 10).await.unwrap().is_empty());

			let retry = store.pending_agent_capacity_retry("agent".into()).await.unwrap().unwrap();

			assert_eq!(retry.attempt, attempt);

			let reopened = SqliteStore::open_test(&path).unwrap();

			assert_eq!(
				reopened.pending_agent_capacity_retry("agent".into()).await.unwrap(),
				Some(retry.clone())
			);
			assert!(
				reopened
					.begin_agent_capacity_retry("agent".into(), event.id, retry.due_at_micros - 1)
					.await
					.is_err()
			);

			reopened
				.begin_agent_capacity_retry("agent".into(), event.id, retry.due_at_micros)
				.await
				.unwrap();

			assert!(
				reopened
					.begin_agent_capacity_retry("agent".into(), event.id, i64::MAX)
					.await
					.is_err()
			);
			assert!(reopened.due_agent_capacity_retries(i64::MAX).await.unwrap().is_empty());

			reopened
				.acknowledge_agent_dispatch("agent".into(), format!("turn-{attempt}"))
				.await
				.unwrap();
		}

		let event = store
			.complete_agent_turn_with_event(
				"agent".into(),
				"turn-3".into(),
				capacity_failure("agent", "turn-3"),
			)
			.await
			.unwrap();

		assert_eq!(event.event_kind, "agent_turn_completed");
		assert!(store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_none());
		assert!(store.due_agent_capacity_retries(i64::MAX).await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn cancellation_new_dispatch_and_unknown_claim_do_not_replay_capacity_retries() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("retry.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();

		for name in ["cancel", "supersede", "unknown", "resolved"] {
			store.create_agent_work_item(item(name, None)).await.unwrap();
			store.bind_agent_thread(name.into(), format!("thread-{name}")).await.unwrap();
			store.begin_agent_dispatch(name.into()).await.unwrap();
			store.acknowledge_agent_dispatch(name.into(), "failed".into()).await.unwrap();

			let event = store
				.complete_agent_turn_with_event(
					name.into(),
					"failed".into(),
					capacity_failure(name, "failed"),
				)
				.await
				.unwrap();

			match name {
				"cancel" => {
					assert!(
						store
							.cancel_agent_capacity_retry("unknown".into(), event.id)
							.await
							.is_err()
					);

					store.cancel_agent_capacity_retry(name.into(), event.id).await.unwrap();
				},
				"resolved" => {
					store
						.set_agent_work_status(name.into(), AgentWorkStatus::Resolved, None)
						.await
						.unwrap();
				},
				"supersede" => {
					store.begin_agent_dispatch(name.into()).await.unwrap();
				},
				_ => {
					store
						.begin_agent_capacity_retry(name.into(), event.id, i64::MAX)
						.await
						.unwrap();
					store.mark_agent_dispatch_unknown(name.into()).await.unwrap();
				},
			}
		}

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();

		assert!(store.due_agent_capacity_retries(i64::MAX).await.unwrap().is_empty());
		assert_eq!(
			store.get_agent_work_item("unknown".into()).await.unwrap().dispatch_state,
			AgentDispatchState::Unknown
		);
	}

	#[tokio::test]
	async fn saved_turn_metrics_require_exact_work_thread_and_turn_after_reopen() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("metrics.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();

		for work in ["chosen", "peer"] {
			store.create_agent_work_item(item(work, None)).await.unwrap();
		}
		for (work, thread, payload_thread, turn, input) in [
			("chosen", "thread-a", "thread-a", "same/turn", 11),
			("chosen", "thread-b", "thread-b", "same/turn", 22),
			("peer", "thread-a", "thread-a", "peer-turn", 33),
			("chosen", "thread-a", "wrong", "mismatch", 44),
		] {
			store.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id:serde_json::json!(["turn/completed",thread,turn]).to_string(),work_item_id:work.into(),event_kind:"agent_turn_completed".into(),
				payload:serde_json::json!({"terminal":{"threadId":payload_thread,"turn":{"id":turn}},"usage":{"input_tokens":input,"output_tokens":2},"threadReadback":{"tokenUsage":{"marker":"observed"},"assistantMessages":"PRIVATE_TRANSCRIPT"}}).to_string(),
			}).await.unwrap();
		}
		for n in 0..50 {
			store
				.enqueue_agent_event(EnqueueAgentEvent {
					source_event_id: format!("later-{n}"),
					work_item_id: "chosen".into(),
					event_kind: "assistant_message".into(),
					payload: "{}".into(),
				})
				.await
				.unwrap();
		}

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();
		let metrics = store
			.read_agent_turn_metrics(
				"chosen".into(),
				"thread-a".into(),
				vec!["same/turn".into(), "peer-turn".into(), "mismatch".into(), "missing".into()],
			)
			.await
			.unwrap();

		assert_eq!(metrics.len(), 1);
		assert_eq!(metrics[0].turn_id, "same/turn");
		assert_eq!(
			serde_json::from_str::<Value>(metrics[0].usage_json.as_ref().unwrap()).unwrap()["input_tokens"],
			11
		);
		assert!(!format!("{metrics:?}").contains("PRIVATE_TRANSCRIPT"));
		assert_eq!(
			store
				.read_agent_turn_metrics(
					"chosen".into(),
					"thread-b".into(),
					vec!["same/turn".into()]
				)
				.await
				.unwrap()
				.len(),
			1
		);
		assert!(
			store
				.read_agent_turn_metrics(
					"chosen".into(),
					"thread-a".into(),
					vec!["turn".into(); 101]
				)
				.await
				.is_err()
		);
	}

	#[tokio::test]
	async fn observations_are_durable_deduplicated_and_never_pending_work() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("agent.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();

		store.create_agent_work_item(item("agent", None)).await.unwrap();

		for sequence in 0..3 {
			let input = EnqueueAgentEvent {
				source_event_id: format!("usage-{sequence}"),
				work_item_id: "agent".into(),
				event_kind: "token_usage".into(),
				payload:
					serde_json::json!({"threadId":"thread","turnId":"turn","sequence":sequence})
						.to_string(),
			};
			let first = store.record_agent_observation(input.clone()).await.unwrap();

			assert_eq!(store.record_agent_observation(input).await.unwrap().id, first.id);
			assert_eq!(first.disposition, Some(AgentDisposition::Resolved));
		}

		assert!(store.list_pending_agent_events(10).await.unwrap().is_empty());
		assert!(store.list_agent_wake_events("agent".into(), 10).await.unwrap().is_empty());
		assert!(store.read_agent_work_events("agent".into(), 10).await.unwrap().is_empty());
		assert_eq!(
			store.get_agent_work_item("agent".into()).await.unwrap().status,
			AgentWorkStatus::Open
		);

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();

		store
			.record_agent_observation(EnqueueAgentEvent {
				source_event_id: "other-thread-usage".into(),
				work_item_id: "agent".into(),
				event_kind: "token_usage".into(),
				payload:
					serde_json::json!({"threadId":"other-thread","turnId":"turn","sequence":999})
						.to_string(),
			})
			.await
			.unwrap();

		let event = store
			.read_agent_usage_observation("agent".into(), "thread".into(), "turn".into())
			.await
			.unwrap()
			.unwrap();

		assert_eq!(serde_json::from_str::<Value>(&event.payload).unwrap()["sequence"], 2);
		assert!(
			store
				.read_agent_usage_observation("agent".into(), "thread".into(), "other".into())
				.await
				.unwrap()
				.is_none()
		);
	}

	#[tokio::test]
	async fn misalignment_reconciliation_preserves_changed_or_invalidated_evidence() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("agent.sqlite3")).unwrap();

		store.create_agent_work_item(item("agent", None)).await.unwrap();
		store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();
		store.restore_agent_misalignment("thread".into(), "failed".into(), None).await.unwrap();

		let review = store.agent_misalignment("agent".into()).await.unwrap().unwrap();

		store.reconcile_agent_misalignment("agent".into(), review.clone(), || false).await.unwrap();

		assert_eq!(store.agent_misalignment("agent".into()).await.unwrap(), Some(review.clone()));

		let calls = AtomicUsize::new(0);

		store
			.reconcile_agent_misalignment("agent".into(), review.clone(), move || {
				calls.fetch_add(1, Ordering::SeqCst) == 0
			})
			.await
			.unwrap();

		assert_eq!(store.agent_misalignment("agent".into()).await.unwrap(), Some(review.clone()));

		store
			.restore_agent_misalignment("thread".into(), "new-failure".into(), None)
			.await
			.unwrap();
		store.reconcile_agent_misalignment("agent".into(), review, || true).await.unwrap();

		let new_review = store.agent_misalignment("agent".into()).await.unwrap().unwrap();

		assert_eq!(new_review.turn_id, "new-failure");

		store.reconcile_agent_misalignment("agent".into(), new_review, || true).await.unwrap();

		assert!(store.agent_misalignment("agent".into()).await.unwrap().is_none());
	}

	#[tokio::test]
	async fn misalignment_continuation_requires_exact_review_and_positive_acknowledgment() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("agent.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();

		store.create_agent_work_item(item("agent", None)).await.unwrap();
		store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();
		store.begin_agent_dispatch("agent".into()).await.unwrap();
		store.acknowledge_agent_dispatch("agent".into(), "failed".into()).await.unwrap();

		let details=serde_json::json!({"detailedExplanation":"Review scope","steer":{"message":"Clarified scope"}}).to_string();

		store
			.record_agent_misalignment("thread".into(), "failed".into(), Some(details))
			.await
			.unwrap();
		store.complete_agent_turn("agent".into(), "failed".into()).await.unwrap();

		let review = store.agent_misalignment("agent".into()).await.unwrap().unwrap();

		store.retire_agent_misalignment_voice("thread".into()).await.unwrap();
		store.reconcile_agent_misalignment("agent".into(), review.clone(), || true).await.unwrap();

		assert_eq!(store.agent_misalignment("agent".into()).await.unwrap(), Some(review.clone()));

		let mut stale = review.clone();

		stale.turn_id = "older".into();

		assert!(
			store
				.begin_agent_misalignment_continuation("agent".into(), stale, "stale".into())
				.await
				.is_err()
		);

		let event = store
			.begin_agent_misalignment_continuation("agent".into(), review.clone(), "first".into())
			.await
			.unwrap();

		assert!(store.agent_misalignment("agent".into()).await.unwrap().is_some());
		assert!(
			store
				.begin_agent_misalignment_continuation(
					"agent".into(),
					review.clone(),
					"duplicate".into()
				)
				.await
				.is_err()
		);

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();

		assert!(
			store
				.begin_agent_misalignment_continuation(
					"agent".into(),
					review.clone(),
					"after-restart".into()
				)
				.await
				.is_err()
		);

		store
			.finish_agent_misalignment_continuation("agent".into(), event, review.clone(), None)
			.await
			.unwrap();

		assert!(store.agent_misalignment("agent".into()).await.unwrap().is_some());

		let event = store
			.begin_agent_misalignment_continuation(
				"agent".into(),
				review.clone(),
				"confirmed".into(),
			)
			.await
			.unwrap();

		assert!(
			store
				.finish_agent_misalignment_continuation(
					"agent".into(),
					event,
					review.clone(),
					Some("failed".into())
				)
				.await
				.is_err()
		);

		store
			.finish_agent_misalignment_continuation(
				"agent".into(),
				event,
				review,
				Some("continued".into()),
			)
			.await
			.unwrap();

		assert!(store.agent_misalignment("agent".into()).await.unwrap().is_none());

		let work = store.get_agent_work_item("agent".into()).await.unwrap();

		assert_eq!(work.dispatch_state, AgentDispatchState::Running);
		assert_eq!(work.active_turn_id.as_deref(), Some("continued"));
	}

	#[tokio::test]
	async fn live_question_provenance_survives_restart_without_promoting_replay() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("agent.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();

		store.create_agent_work_item(item("agent", None)).await.unwrap();
		store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();

		let record = |id: &str| {
			vec![(
				id.to_owned(),
				serde_json::json!({"id":id,"title":"Question","options":[]}).to_string(),
			)]
		};

		store
			.record_agent_async_questions(
				"thread".into(),
				"turn".into(),
				"history".into(),
				record("old"),
			)
			.await
			.unwrap();
		store
			.record_live_agent_async_questions(
				"thread".into(),
				"turn".into(),
				"history".into(),
				record("old"),
			)
			.await
			.unwrap();
		store
			.record_live_agent_async_questions(
				"thread".into(),
				"turn".into(),
				"live".into(),
				record("new"),
			)
			.await
			.unwrap();
		store
			.record_live_agent_async_questions(
				"thread".into(),
				"turn".into(),
				"live".into(),
				record("new"),
			)
			.await
			.unwrap();

		let questions = store.read_agent_async_questions("agent".into()).await.unwrap();

		assert_eq!(questions.len(), 2);
		assert!(!questions[0].arrived_live);
		assert!(questions[1].arrived_live);

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();

		store.refresh_agent_async_projection("thread".into()).await.unwrap();

		assert!(store.read_agent_async_questions("agent".into()).await.unwrap().is_empty());
		assert!(
			store
				.replace_agent_async_projection(
					"agent".into(),
					"thread".into(),
					None,
					questions,
					vec![]
				)
				.await
				.unwrap()
		);

		let mut questions = store.read_agent_async_questions("agent".into()).await.unwrap();

		assert!(!questions[0].arrived_live);
		assert!(questions[1].arrived_live);

		// Input provenance is not authority: a changed native question is history.
		questions[1].question_json =
			serde_json::json!({"id":"new","title":"Changed","options":[]}).to_string();

		store.refresh_agent_async_projection("thread".into()).await.unwrap();
		store
			.replace_agent_async_projection(
				"agent".into(),
				"thread".into(),
				None,
				questions,
				vec![],
			)
			.await
			.unwrap();

		assert!(
			store
				.read_agent_async_questions("agent".into())
				.await
				.unwrap()
				.iter()
				.all(|q| !q.arrived_live)
		);
		assert!(
			store
				.skip_agent_async_question("agent".into(), "thread".into(), "new".into())
				.await
				.unwrap()
		);

		store.resolve_agent_async_questions("thread".into(), vec!["old".into()]).await.unwrap();

		assert!(store.read_agent_async_questions("agent".into()).await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn new_prompt_retires_questions_without_replay_or_answer_side_effects() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("agent.sqlite3")).unwrap();

		store.create_agent_work_item(item("agent", None)).await.unwrap();
		store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();

		record_question(&store, "turn", "item", "q1").await;

		let answer = EnqueueAgentEvent {
			source_event_id: "reply".into(),
			work_item_id: "agent".into(),
			event_kind: "user_message".into(),
			payload: r#"{"text":"Answer","asyncQuestionReply":true}"#.into(),
		};

		store.enqueue_agent_event(answer).await.unwrap();

		assert_eq!(store.read_agent_async_questions("agent".into()).await.unwrap().len(), 1);

		let prompt = EnqueueAgentEvent {
			source_event_id: "prompt".into(),
			work_item_id: "agent".into(),
			event_kind: "user_message".into(),
			payload: r#"{"text":"New work"}"#.into(),
		};

		store.enqueue_agent_event(prompt.clone()).await.unwrap();

		record_question(&store, "turn", "item", "q1").await;

		assert!(store.read_agent_async_questions("agent".into()).await.unwrap().is_empty());

		record_question(&store, "turn", "item2", "q2").await;

		store.enqueue_agent_event(prompt).await.unwrap();

		assert_eq!(store.read_agent_async_questions("agent".into()).await.unwrap().len(), 1);

		// Queued prompt delivery retires questions that arrived while waiting.
		let event = store
			.read_agent_work_events("agent".into(), 10)
			.await
			.unwrap()
			.into_iter()
			.find(|event| event.source_event_id == "prompt")
			.unwrap();

		store.begin_agent_dispatch_with_events("agent".into(), vec![event.id]).await.unwrap();

		assert!(store.read_agent_async_questions("agent".into()).await.unwrap().is_empty());

		store.acknowledge_agent_dispatch("agent".into(), "active".into()).await.unwrap();

		record_question(&store, "active", "item3", "q3").await;

		let rejected = store
			.begin_agent_steer(
				"agent".into(),
				"active".into(),
				"rejected".into(),
				r#"{"text":"New work"}"#.into(),
			)
			.await
			.unwrap();

		store.finish_agent_steer(rejected, false).await.unwrap();

		assert_eq!(store.read_agent_async_questions("agent".into()).await.unwrap().len(), 1);

		let reply = store
			.begin_agent_steer(
				"agent".into(),
				"active".into(),
				"reply".into(),
				r#"{"text":"Answer","asyncQuestionReply":true}"#.into(),
			)
			.await
			.unwrap();

		store.finish_agent_steer(reply, true).await.unwrap();

		assert_eq!(store.read_agent_async_questions("agent".into()).await.unwrap().len(), 1);

		let accepted = store
			.begin_agent_steer(
				"agent".into(),
				"active".into(),
				"accepted".into(),
				r#"{"text":"New work"}"#.into(),
			)
			.await
			.unwrap();

		store.finish_agent_steer(accepted, true).await.unwrap();

		assert!(store.read_agent_async_questions("agent".into()).await.unwrap().is_empty());
	}
	#[tokio::test]
	async fn async_answers_only_dispatch_once_to_the_exact_owner_and_never_wake() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("agent.sqlite3")).unwrap();

		store.create_agent_work_item(item("agent", None)).await.unwrap();
		store.create_agent_work_item(item("worker", Some("agent"))).await.unwrap();
		store.bind_agent_thread("agent".into(), "agent-thread".into()).await.unwrap();
		store.bind_agent_thread("worker".into(), "worker-thread".into()).await.unwrap();

		let event = store
			.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id: "answer-1".into(),
				work_item_id: "worker".into(),
				event_kind: "async_question_answer".into(),
				payload: r#"{"text":"Europe","source":"user"}"#.into(),
			})
			.await
			.unwrap();

		assert!(store.list_undelivered_agent_events(10).await.unwrap().is_empty());
		assert!(store.list_agent_wake_events("agent".into(), 10).await.unwrap().is_empty());
		assert!(
			store.begin_agent_dispatch_with_events("agent".into(), vec![event.id]).await.is_err()
		);
		assert_eq!(
			store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
			AgentDispatchState::Idle
		);

		store.begin_agent_dispatch_with_events("worker".into(), vec![event.id]).await.unwrap();
		store.acknowledge_agent_dispatch("worker".into(), "turn-1".into()).await.unwrap();

		let receipt = store.get_agent_inbox_event(event.id).await.unwrap();

		assert_eq!(receipt.delivered_turn_id.as_deref(), Some("turn-1"));
		assert_eq!(receipt.work_item_id, "worker");

		store.complete_agent_turn("worker".into(), "turn-1".into()).await.unwrap();

		assert!(
			store.begin_agent_dispatch_with_events("worker".into(), vec![event.id]).await.is_err()
		);
		assert_eq!(
			store.get_agent_work_item("worker".into()).await.unwrap().dispatch_state,
			AgentDispatchState::Idle
		);
		assert!(store.list_agent_wake_events("agent".into(), 10).await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn occupied_input_admission_preserves_replay_and_other_conversations() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("agent.sqlite3")).unwrap();

		for id in ["agent", "other"] {
			store.create_agent_work_item(item(id, None)).await.unwrap();
		}

		let input = |work: &str, key: &str| EnqueueAgentEvent {
			source_event_id: key.into(),
			work_item_id: work.into(),
			event_kind: "user_message".into(),
			payload: r#"{"text":"continue"}"#.into(),
		};
		let accepted = store.enqueue_agent_event(input("agent", "accepted")).await.unwrap();

		store.record_agent_thread_in_use("agent".into(), "Open elsewhere".into()).await.unwrap();

		assert_eq!(store.enqueue_agent_event(input("agent", "accepted")).await.unwrap(), accepted);
		assert!(matches!(
			store.enqueue_agent_event(input("agent", "new")).await,
			Err(StoreError::AgentThreadInUse)
		));

		store.enqueue_agent_event(input("other", "unrelated")).await.unwrap();

		assert_eq!(store.list_undelivered_agent_events(10).await.unwrap().len(), 2);

		let blocked = store.list_agent_thread_in_use_events().await.unwrap();

		assert_eq!(blocked.len(), 1);
		assert_eq!(blocked[0].work_item_id, "agent");

		store.resolve_agent_delivery_failure("agent".into()).await.unwrap();

		assert!(store.list_agent_thread_in_use_events().await.unwrap().is_empty());

		store.enqueue_agent_event(input("agent", "new")).await.unwrap();

		assert_eq!(store.list_undelivered_agent_events(10).await.unwrap().len(), 3);
	}

	#[tokio::test]
	async fn delivery_attention_recovers_without_consuming_saved_user_input() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("agent.sqlite3")).unwrap();

		store.create_agent_work_item(item("agent", None)).await.unwrap();
		store
			.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id: "user-1".into(),
				work_item_id: "agent".into(),
				event_kind: "user_message".into(),
				payload: r#"{"text":"continue"}"#.into(),
			})
			.await
			.unwrap();
		store.record_agent_delivery_failure("agent".into(), "Unavailable".into()).await.unwrap();
		store.record_agent_thread_in_use("agent".into(), "Open elsewhere".into()).await.unwrap();
		store.record_agent_thread_in_use("agent".into(), "Open elsewhere".into()).await.unwrap();

		assert!(
			store
				.list_pending_agent_events(10)
				.await
				.unwrap()
				.iter()
				.any(|e| e.event_kind == "thread_in_use_needs_attention")
		);
		assert_eq!(store.list_pending_agent_events(10).await.unwrap().len(), 2);

		store.resolve_agent_delivery_failure("agent".into()).await.unwrap();

		let pending = store.list_pending_agent_events(10).await.unwrap();

		assert_eq!(pending.len(), 1);
		assert_eq!(pending[0].event_kind, "user_message");
		assert!(pending[0].delivered_turn_id.is_none());

		store.record_agent_delivery_failure("agent".into(), "Later failure".into()).await.unwrap();

		assert_eq!(store.read_agent_work_events("agent".into(), 10).await.unwrap().len(), 4);
	}

	#[tokio::test]
	async fn redispatch_invalidates_manager_acceptance_and_records_instruction_atomically() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("agent.sqlite3")).unwrap();

		store.create_agent_work_item(item("agent", None)).await.unwrap();
		store.create_agent_manager(item("manager", Some("agent")), None).await.unwrap();
		store.bind_agent_thread("manager".into(), "manager-thread".into()).await.unwrap();
		store
			.set_agent_work_status("manager".into(), AgentWorkStatus::Resolved, None)
			.await
			.unwrap();
		store
			.begin_agent_dispatch_with_input(
				"manager".into(),
				vec![],
				Some("Check the revised result".into()),
			)
			.await
			.unwrap();

		assert_eq!(
			store.get_agent_work_item("manager".into()).await.unwrap().status,
			AgentWorkStatus::Open
		);
		assert!(
			store
				.begin_agent_dispatch_with_input(
					"manager".into(),
					vec![],
					Some("Do not duplicate".into())
				)
				.await
				.is_err()
		);

		store.acknowledge_agent_dispatch("manager".into(), "new-turn".into()).await.unwrap();

		let history = store.read_agent_work_events("manager".into(), 10).await.unwrap();

		assert_eq!(history.len(), 1);
		assert_eq!(history[0].event_kind, "work_instruction");
		assert_eq!(history[0].delivered_turn_id.as_deref(), Some("new-turn"));
		assert_eq!(
			serde_json::from_str::<Value>(&history[0].payload).unwrap()["text"],
			"Check the revised result"
		);
		assert!(store.list_undelivered_agent_events(10).await.unwrap().is_empty());

		store.complete_agent_turn("manager".into(), "new-turn".into()).await.unwrap();

		assert_eq!(
			store.get_agent_work_item("manager".into()).await.unwrap().status,
			AgentWorkStatus::Open
		);
	}

	#[tokio::test]
	async fn connection_attention_closes_and_rearms_without_resolving_work() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("agent.sqlite3")).unwrap();

		store.create_agent_work_item(item("agent", None)).await.unwrap();
		store
			.record_agent_connection_failure("agent".into(), "Refresh quota".into())
			.await
			.unwrap();
		store
			.record_agent_connection_failure("agent".into(), "Retry pending".into())
			.await
			.unwrap();

		assert_eq!(store.read_agent_work_events("agent".into(), 10).await.unwrap().len(), 2);

		store.resolve_agent_connection_failure("agent".into()).await.unwrap();

		let saved = store.read_agent_work_events("agent".into(), 10).await.unwrap();

		assert_eq!(saved[0].disposition, Some(AgentDisposition::Resolved));
		assert_eq!(
			store.get_agent_work_item("agent".into()).await.unwrap().status,
			AgentWorkStatus::Open
		);

		store
			.record_agent_connection_failure("agent".into(), "Later failure".into())
			.await
			.unwrap();

		let saved = store.read_agent_work_events("agent".into(), 10).await.unwrap();

		assert_eq!(saved.len(), 3);
		assert_eq!(saved.iter().filter(|event| event.disposition.is_none()).count(), 1);
	}

	#[tokio::test]
	async fn agent_request_receipts_and_handled_inputs_leave_work_judgment_and_newer_events_intact()
	{
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("agent.sqlite3")).unwrap();

		store.create_agent_work_item(item("agent", None)).await.unwrap();
		store.bind_agent_thread("agent".into(), "thread".into()).await.unwrap();
		store
			.set_agent_work_status("agent".into(), AgentWorkStatus::Wait, Some(100))
			.await
			.unwrap();

		let initial = store.get_agent_work_item("agent".into()).await.unwrap();

		for kind in ["permission_pending", "user_input_pending", "server_request_pending"] {
			let request = store
				.enqueue_agent_event(EnqueueAgentEvent {
					source_event_id: kind.into(),
					work_item_id: "agent".into(),
					event_kind: kind.into(),
					payload: "{}".into(),
				})
				.await
				.unwrap();
			let acknowledged = store.acknowledge_agent_request_event(request.id).await.unwrap();

			assert_eq!(acknowledged.disposition, Some(AgentDisposition::Resolved));
			assert!(store.acknowledge_agent_request_event(request.id).await.is_err());
		}

		assert_eq!(store.get_agent_work_item("agent".into()).await.unwrap(), initial);

		let mut retained = Vec::new();

		for (index, status) in [(1, "failed"), (2, "interrupted"), (3, "completed")] {
			let input = store
				.enqueue_agent_event(EnqueueAgentEvent {
					source_event_id: format!("user:{index}"),
					work_item_id: "agent".into(),
					event_kind: "user_message".into(),
					payload: "user input".into(),
				})
				.await
				.unwrap();

			assert!(store.acknowledge_agent_request_event(input.id).await.is_err());

			store.begin_agent_dispatch_with_events("agent".into(), vec![input.id]).await.unwrap();

			let turn = format!("turn:{index}");

			store.acknowledge_agent_dispatch("agent".into(), turn.clone()).await.unwrap();

			if status == "completed" {
				let newer = store
					.enqueue_agent_event(EnqueueAgentEvent {
						source_event_id: "newer-user".into(),
						work_item_id: "agent".into(),
						event_kind: "user_message".into(),
						payload: "newer input".into(),
					})
					.await
					.unwrap();

				retained.push(newer.id);
			} else {
				retained.push(input.id);
			}

			store
				.complete_agent_turn_with_event(
					"agent".into(),
					turn,
					EnqueueAgentEvent {
						source_event_id: format!("terminal:{index}"),
						work_item_id: "agent".into(),
						event_kind: "agent_turn_completed".into(),
						payload: serde_json::json!({"terminal":{"turn":{"status":status}}})
							.to_string(),
					},
				)
				.await
				.unwrap();
		}

		let pending = store.list_pending_agent_events(100).await.unwrap();

		assert_eq!(pending.iter().map(|event| event.id).collect::<Vec<_>>(), retained);

		let work = store.get_agent_work_item("agent".into()).await.unwrap();

		assert_eq!(work.status, AgentWorkStatus::Wait);
		assert_eq!(work.next_check_at_micros, Some(100));
	}

	#[tokio::test]
	async fn agent_due_notifications_filter_before_limit_and_user_messages_can_wake() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("agent.sqlite3")).unwrap();

		for id in ["first", "second"] {
			store.create_agent_work_item(item(id, None)).await.unwrap();
			store.set_agent_work_status(id.into(), AgentWorkStatus::Wait, Some(100)).await.unwrap();
		}

		store
			.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id: serde_json::json!(["followup_due", "first", 100]).to_string(),
				work_item_id: "first".into(),
				event_kind: "followup_due".into(),
				payload: "{}".into(),
			})
			.await
			.unwrap();

		assert_eq!(
			store.list_unnotified_due_agent_work_items(100, 1).await.unwrap()[0].id,
			"second"
		);

		store
			.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id: "user-command:1".into(),
				work_item_id: "first".into(),
				event_kind: "user_message".into(),
				payload: "Hello Agent".into(),
			})
			.await
			.unwrap();

		assert!(
			store
				.list_undelivered_agent_events(10)
				.await
				.unwrap()
				.iter()
				.any(|event| event.event_kind == "user_message")
		);
	}

	#[tokio::test]
	async fn agent_reads_filter_before_limits_and_terminal_receipts_do_not_change_judgment() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("agent.sqlite3")).unwrap();

		store.create_agent_work_item(item("agent", None)).await.unwrap();
		store.begin_agent_thread_creation("agent".into()).await.unwrap();

		assert!(store.begin_agent_thread_creation("agent".into()).await.is_err());

		store
			.acknowledge_agent_thread_creation("agent".into(), "opaque-thread".into())
			.await
			.unwrap();

		for (source, kind) in
			[("unrelated-1", "other"), ("unrelated-2", "other"), ("wake", "automation_result")]
		{
			store
				.enqueue_agent_event(EnqueueAgentEvent {
					source_event_id: source.into(),
					work_item_id: "agent".into(),
					event_kind: kind.into(),
					payload: String::new(),
				})
				.await
				.unwrap();
		}

		let event = store.list_undelivered_agent_events(1).await.unwrap().remove(0);

		assert_eq!(event.source_event_id, "wake");

		store
			.set_agent_work_status("agent".into(), AgentWorkStatus::Wait, Some(100))
			.await
			.unwrap();
		store.begin_agent_dispatch_with_events("agent".into(), vec![event.id]).await.unwrap();
		store.acknowledge_agent_dispatch("agent".into(), "turn-1".into()).await.unwrap();

		assert!(store.list_undelivered_agent_events(1).await.unwrap().is_empty());
		assert_eq!(
			store.list_agent_events_for_turn("turn-1".into(), 1).await.unwrap()[0].id,
			event.id
		);

		let receipt = store
			.complete_agent_turn_with_event(
				"agent".into(),
				"turn-1".into(),
				EnqueueAgentEvent {
					source_event_id: "agent-terminal".into(),
					work_item_id: "agent".into(),
					event_kind: "agent_turn_completed".into(),
					payload: "agent reply".into(),
				},
			)
			.await
			.unwrap();

		assert_eq!(receipt.disposition, Some(AgentDisposition::Resolved));

		let work = store.get_agent_work_item("agent".into()).await.unwrap();

		assert_eq!(work.status, AgentWorkStatus::Wait);
		assert_eq!(work.next_check_at_micros, Some(100));
		assert_eq!(store.list_pending_agent_events(100).await.unwrap().len(), 3);
		assert!(matches!(
			store.read_agent_snapshot(1, 1, 1).await.unwrap(),
			AgentStoreSnapshot::CapacityExceeded { pending_events: 3, .. }
		));
	}

	#[tokio::test]
	async fn agent_thread_creation_unknown_survives_reopen_and_new_task_turn_revokes_old_acceptance()
	 {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("agent.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();

		store.create_agent_work_item(item("unknown", None)).await.unwrap();
		store.begin_agent_thread_creation("unknown".into()).await.unwrap();
		store.mark_agent_dispatch_unknown("unknown".into()).await.unwrap();

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();

		assert!(store.begin_agent_thread_creation("unknown".into()).await.is_err());

		let mut task = item("task", None);

		task.kind = AgentWorkKind::Task;

		store.create_agent_work_item(task).await.unwrap();
		store.begin_agent_thread_creation("task".into()).await.unwrap();
		store.acknowledge_agent_thread_creation("task".into(), "task-thread".into()).await.unwrap();
		store.set_agent_work_status("task".into(), AgentWorkStatus::Resolved, None).await.unwrap();

		let dispatched = store.begin_agent_dispatch("task".into()).await.unwrap();

		assert_eq!(dispatched.status, AgentWorkStatus::Open);
	}

	#[tokio::test]
	async fn agent_transport_close_preserves_running_turn_as_unknown_across_reopen() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("agent.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();

		store.create_agent_work_item(item("worker", None)).await.unwrap();
		store.bind_agent_thread("worker".into(), "opaque-thread".into()).await.unwrap();
		store.begin_agent_dispatch("worker".into()).await.unwrap();
		store
			.acknowledge_agent_dispatch("worker".into(), "exact-running-turn".into())
			.await
			.unwrap();

		let unknown = store.mark_agent_dispatch_unknown("worker".into()).await.unwrap();

		assert_eq!(unknown.dispatch_state, AgentDispatchState::Unknown);
		assert_eq!(unknown.active_turn_id.as_deref(), Some("exact-running-turn"));

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();

		assert_eq!(store.get_agent_work_item("worker".into()).await.unwrap(), unknown);
		assert!(store.begin_agent_dispatch("worker".into()).await.is_err());
		assert!(
			store.reconcile_agent_dispatch("worker".into(), "different-turn".into()).await.is_err()
		);

		let reconciled = store
			.reconcile_agent_dispatch("worker".into(), "exact-running-turn".into())
			.await
			.unwrap();

		assert_eq!(reconciled.dispatch_state, AgentDispatchState::Running);
		assert!(store.begin_agent_dispatch("worker".into()).await.is_err());

		store.revalidate().await.unwrap();
	}

	#[tokio::test]
	async fn agent_dispatch_delivery_and_unknown_state_survive_restart_without_replay() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("agent.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();

		store.create_agent_work_item(item("agent", None)).await.unwrap();
		store.bind_agent_thread("agent".into(), "opaque-thread".into()).await.unwrap();

		let event = store
			.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id: "wake:1".into(),
				work_item_id: "agent".into(),
				event_kind: "automation_result".into(),
				payload: String::new(),
			})
			.await
			.unwrap();

		assert!(
			store
				.begin_agent_dispatch_with_events("agent".into(), vec![event.id, -1])
				.await
				.is_err()
		);
		assert_eq!(
			store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
			AgentDispatchState::Idle
		);
		assert_eq!(store.list_pending_agent_events(10).await.unwrap()[0].delivered_turn_id, None);

		store.begin_agent_dispatch_with_events("agent".into(), vec![event.id]).await.unwrap();

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();

		assert_eq!(
			store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
			AgentDispatchState::Dispatching
		);
		assert!(store.begin_agent_dispatch("agent".into()).await.is_err());
		assert_eq!(
			store.list_pending_agent_events(10).await.unwrap()[0].delivered_turn_id,
			Some(String::new())
		);

		store.mark_agent_dispatch_unknown("agent".into()).await.unwrap();

		assert!(store.begin_agent_dispatch("agent".into()).await.is_err());

		store.reconcile_agent_dispatch("agent".into(), "opaque-turn".into()).await.unwrap();

		assert!(store.complete_agent_turn("agent".into(), "wrong-turn".into()).await.is_err());

		let terminal = EnqueueAgentEvent {
			source_event_id: "completion:1".into(),
			work_item_id: "agent".into(),
			event_kind: "turn_completed".into(),
			payload: "positive exact-turn evidence".into(),
		};

		store
			.complete_agent_turn_with_event("agent".into(), "opaque-turn".into(), terminal)
			.await
			.unwrap();

		assert_eq!(
			store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
			AgentDispatchState::Idle
		);

		let pending = store.list_pending_agent_events(10).await.unwrap();

		assert_eq!(pending.len(), 2);
		assert_eq!(pending[0].delivered_turn_id.as_deref(), Some("opaque-turn"));
		assert!(pending.iter().all(|event| event.disposition.is_none()));

		store.revalidate().await.unwrap();
	}

	#[tokio::test]
	async fn agent_graph_rejects_cycles_and_preserves_opaque_binding() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("agent.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();

		store.create_agent_work_item(item("goal", None)).await.unwrap();
		store.create_agent_work_item(item("a", Some("goal"))).await.unwrap();
		store.create_agent_work_item(item("b", Some("goal"))).await.unwrap();
		store.create_agent_work_item(item("c", Some("goal"))).await.unwrap();
		store.add_agent_dependency("a".into(), "b".into()).await.unwrap();
		store.add_agent_dependency("b".into(), "c".into()).await.unwrap();

		assert!(store.add_agent_dependency("c".into(), "a".into()).await.is_err());
		assert!(store.add_agent_dependency("a".into(), "a".into()).await.is_err());

		store
			.with_connection(|connection| {
				assert!(
					connection
						.execute("INSERT INTO agent_dependencies VALUES ('c', 'a')", [])
						.is_err()
				);
				assert!(
					connection
						.execute(
							"UPDATE agent_work_items SET parent_goal_id = 'c' WHERE id = 'goal'",
							[]
						)
						.is_err()
				);

				Ok(())
			})
			.unwrap();

		assert!(store.create_agent_work_item(item("missing", Some("unknown"))).await.is_err());

		let opaque = "thread:provider/opaque not-a-uuid".to_owned();

		store.bind_agent_thread("a".into(), opaque.clone()).await.unwrap();

		assert!(store.bind_agent_thread("a".into(), "other".into()).await.is_err());
		assert!(store.bind_agent_thread("b".into(), opaque.clone()).await.is_err());

		drop(store);

		let reopened = SqliteStore::open_test(&path).unwrap();

		assert_eq!(reopened.list_agent_dependencies().await.unwrap().len(), 2);
		assert_eq!(
			reopened
				.list_agent_work_items()
				.await
				.unwrap()
				.into_iter()
				.find(|item| item.id == "a")
				.unwrap()
				.codex_thread_id,
			Some(opaque)
		);
	}

	#[tokio::test]
	async fn agent_inbox_survives_restart_deduplicates_and_disposes_exactly_once() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("agent.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();

		store.create_agent_work_item(item("goal", None)).await.unwrap();

		let input = EnqueueAgentEvent {
			source_event_id: "source:event:1".into(),
			work_item_id: "goal".into(),
			event_kind: "thread_completed".into(),
			payload: "result".into(),
		};
		let first = store.enqueue_agent_event(input.clone()).await.unwrap();

		assert_eq!(first, store.enqueue_agent_event(input.clone()).await.unwrap());

		let mut changed = input.clone();

		changed.payload = "different".into();

		assert!(matches!(
			store.enqueue_agent_event(changed).await,
			Err(StoreError::IdempotencyConflict)
		));

		let mut second_input = input.clone();

		second_input.source_event_id = "source:event:2".into();

		let second = store.enqueue_agent_event(second_input).await.unwrap();

		assert_eq!(store.list_pending_agent_events(1).await.unwrap(), vec![first.clone()]);

		drop(store);

		let reopened = SqliteStore::open_test(&path).unwrap();

		assert_eq!(reopened.list_pending_agent_events(100).await.unwrap().len(), 2);

		let (one, two) = tokio::join!(
			reopened.dispose_agent_event(
				first.id,
				AgentDisposition::Wait,
				"check again".into(),
				Some(100)
			),
			reopened.dispose_agent_event(
				first.id,
				AgentDisposition::Wait,
				"duplicate processor".into(),
				Some(100)
			)
		);

		assert_ne!(one.is_ok(), two.is_ok());
		assert_eq!(reopened.list_pending_agent_events(100).await.unwrap(), vec![second]);
		assert_eq!(reopened.list_due_agent_work_items(100, 100).await.unwrap().len(), 1);
		assert!(reopened.enqueue_agent_event(input).await.unwrap().disposition.is_some());

		reopened.revalidate().await.unwrap();
	}
}
