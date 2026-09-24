//! Durable Chief work and inbox facts. The caller owns planning and judgment.

use rusqlite::{Connection, OptionalExtension as _, Row, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

mod capacity;
mod steer;
pub use capacity::ChiefCapacityRetry;

use crate::{DatabaseError, SqliteStore, StoreError, error::sqlite_error, unix_micros};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefWorkKind {
	Goal,
	Task,
}

impl ChiefWorkKind {
	fn as_str(self) -> &'static str {
		match self {
			Self::Goal => "goal",
			Self::Task => "task",
		}
	}
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefDisposition {
	Resolved,
	FollowUp,
	Wait,
	UserDecision,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefWorkStatus {
	Open,
	Resolved,
	FollowUp,
	Wait,
	UserDecision,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefDispatchState {
	#[default]
	Idle,
	Dispatching,
	Running,
	Unknown,
}

impl ChiefWorkStatus {
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

impl ChiefDisposition {
	fn as_str(self) -> &'static str {
		match self {
			Self::Resolved => "resolved",
			Self::FollowUp => "follow_up",
			Self::Wait => "wait",
			Self::UserDecision => "user_decision",
		}
	}
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChiefWorkItem {
	pub id: String,
	pub parent_goal_id: Option<String>,
	pub kind: ChiefWorkKind,
	pub title: String,
	pub instructions: String,
	/// An opaque app-server identity. No UUID or filesystem interpretation is permitted.
	pub codex_thread_id: Option<String>,
	pub dispatch_state: ChiefDispatchState,
	pub active_turn_id: Option<String>,
	pub status: ChiefWorkStatus,
	pub next_check_at_micros: Option<i64>,
	pub created_at_micros: i64,
	pub updated_at_micros: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChiefDependency {
	pub work_item_id: String,
	pub depends_on_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EnqueueChiefEvent {
	/// Stable identity supplied by the event source; retries must reuse this value.
	pub source_event_id: String,
	pub work_item_id: String,
	pub event_kind: String,
	pub payload: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChiefInboxEvent {
	pub id: i64,
	pub source_event_id: String,
	pub work_item_id: String,
	pub event_kind: String,
	pub payload: String,
	pub created_at_micros: i64,
	pub disposition: Option<ChiefDisposition>,
	pub disposition_note: Option<String>,
	pub disposed_at_micros: Option<i64>,
	/// Empty while dispatch is claimed or unknown; the exact turn ID after acknowledgment.
	/// This delivery fence never disposes the event.
	pub delivered_turn_id: Option<String>,
}

/// One atomic bounded read for a public projection.
pub enum ChiefStoreSnapshot {
	Complete {
		managers: Vec<String>,
		workspaces: Vec<(String, String, String)>,
		work_items: Vec<ChiefWorkItem>,
		dependencies: Vec<ChiefDependency>,
		pending_events: Vec<ChiefInboxEvent>,
	},
	CapacityExceeded {
		work_items: u64,
		dependencies: u64,
		pending_events: u64,
	},
}

/// Saved usage fields for one exact native completed turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefTurnMetrics {
	/// Exact native turn identity.
	pub turn_id: String,
	/// Acknowledged whole-turn counter delta, absent when not known.
	pub usage_json: Option<String>,
	/// Latest provider usage observation retained with this completion.
	pub observation_json: Option<String>,
}

impl SqliteStore {
	/// Read only usage fields from exact completed-turn receipts, without transcript text.
	pub async fn read_chief_turn_metrics(
		&self,
		work: String,
		thread: String,
		turns: Vec<String>,
	) -> Result<Vec<ChiefTurnMetrics>, StoreError> {
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
			connection.prepare("SELECT requested.value,json_extract(e.payload,'$.usage'),json_extract(e.payload,'$.threadReadback.tokenUsage') FROM json_each(?3) requested JOIN chief_inbox_events e ON e.source_event_id=json_array('turn/completed',?2,requested.value) WHERE e.work_item_id=?1 AND e.event_kind IN ('chief_turn_completed','worker_turn_completed') AND json_extract(e.payload,'$.terminal.threadId')=?2 AND json_extract(e.payload,'$.terminal.turn.id')=requested.value")
				.map_err(sqlite_error)?.query_map(params![work,thread,turns],|row|Ok(ChiefTurnMetrics {turn_id:row.get(0)?,usage_json:row.get(1)?,observation_json:row.get(2)?}))
				.map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(|error|sqlite_error(error).into())
		}).await
	}

	/// Read the latest bounded work events in chronological order.
	pub async fn read_chief_work_events(
		&self,
		work_id: String,
		limit: usize,
	) -> Result<Vec<ChiefInboxEvent>, StoreError> {
		self.read_chief_work_events_before(work_id, None, limit).await
	}

	/// Read an immutable page strictly before an event identity.
	pub async fn read_chief_work_events_before(
		&self,
		work_id: String,
		before: Option<i64>,
		limit: usize,
	) -> Result<Vec<ChiefInboxEvent>, StoreError> {
		let limit = page_limit(limit)?;
		self.run(move |connection| {
			read_work(connection, &work_id)?;
			connection.prepare("SELECT * FROM (SELECT * FROM chief_inbox_events WHERE work_item_id = ?1 AND event_kind NOT IN ('turn_execution','native_task_settings','token_usage','response_usage','live_reviewer_attempt','live_reviewer_result','native_task_permissions','permission_selection','permission_selection_result','permission_selection_observation','native_task_plugins','plugin_selection','plugin_selection_result','plugin_selection_observation','hook_setting_attempt','hook_setting_result','hook_setting_observation') AND (?3 IS NULL OR id < ?3) ORDER BY id DESC LIMIT ?2) ORDER BY id")
				.map_err(sqlite_error)?.query_map(params![work_id, limit, before], event_row)
				.map_err(sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(|error| sqlite_error(error).into())
		}).await
	}

	/// Read exact-work unconfirmed inputs, independently of transcript pages and scheduler claims.
	pub async fn read_chief_unconfirmed_inputs(
		&self,
		work_id: String,
		after: Option<i64>,
		limit: usize,
	) -> Result<Vec<ChiefInboxEvent>, StoreError> {
		let limit = page_limit(limit)?;
		if after.is_some_and(|id| id < 1) {
			return Err(StoreError::InvalidInput("invalid input receipt cursor"));
		}
		self.run(move |connection| {
			read_work(connection, &work_id)?;
			connection.prepare("SELECT * FROM chief_inbox_events WHERE work_item_id=?1 AND event_kind IN ('user_message','async_question_answer','work_instruction') AND disposition IS NULL AND (delivered_turn_id IS NULL OR delivered_turn_id='') AND (?2 IS NULL OR id>?2) ORDER BY id LIMIT ?3")
				.map_err(sqlite_error)?.query_map(params![work_id,after,limit],event_row)
				.map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(|error|sqlite_error(error).into())
		}).await
	}

	/// Read final history and partial output from one database snapshot.
	pub async fn read_chief_transcript(
		&self,
		id: String,
		before: Option<i64>,
		limit: usize,
	) -> Result<(Vec<ChiefInboxEvent>, Vec<crate::ChiefLiveOutput>), StoreError> {
		let limit = page_limit(limit)?;
		self.run(move |connection| {
            let tx=connection.transaction().map_err(sqlite_error)?;
            read_work(&tx,&id)?;
            let events=tx.prepare("SELECT * FROM (SELECT e.* FROM chief_inbox_events e WHERE work_item_id=?1 AND event_kind NOT IN ('turn_execution','native_task_settings','token_usage','response_usage','live_reviewer_attempt','live_reviewer_result','native_task_permissions','permission_selection','permission_selection_result','permission_selection_observation','native_task_plugins','plugin_selection','plugin_selection_result','plugin_selection_observation','hook_setting_attempt','hook_setting_result','hook_setting_observation') AND (event_kind<>'activity_started' OR NOT EXISTS(SELECT 1 FROM chief_inbox_events c WHERE c.source_event_id=json_array('activity',e.work_item_id,json_extract(e.payload,'$.turn_id'),json_extract(e.payload,'$.item_id'),'completed'))) AND (event_kind<>'steer_pending' OR disposition IS NULL) AND (?3 IS NULL OR id<?3) ORDER BY id DESC LIMIT ?2) ORDER BY id").map_err(sqlite_error)?.query_map(params![id,limit,before],event_row).map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(sqlite_error)?;
            let live=if before.is_none() {crate::chief_output::read_live(&tx,&id)?} else {vec![]};
            tx.commit().map_err(sqlite_error)?;
            Ok((events,live))
        }).await
	}

	/// Return executable manager identities, including the original root.
	pub async fn chief_manager_ids(&self) -> Result<Vec<String>, StoreError> {
		self.run(|connection| connection.prepare("SELECT id FROM chief_work_items WHERE parent_goal_id IS NULL UNION SELECT work_id FROM chief_managers").map_err(sqlite_error)?
            .query_map([],|row|row.get(0)).map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(|error|sqlite_error(error).into())).await
	}

	/// Read persisted project scopes owned by managers.
	pub async fn chief_workspaces(&self) -> Result<Vec<(String, String, String)>, StoreError> {
		self.run(|connection| {
			connection
				.prepare("SELECT chief_id,name,directory FROM chief_workspaces ORDER BY chief_id")
				.map_err(sqlite_error)?
				.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
				.map_err(sqlite_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(|error| sqlite_error(error).into())
		})
		.await
	}

	/// Read one exact inbox record for a source-bound detail request.
	pub async fn get_chief_inbox_event(&self, id: i64) -> Result<ChiefInboxEvent, StoreError> {
		self.run(move |connection| read_event(connection, id)).await
	}

	pub async fn read_chief_snapshot(
		&self,
		work_limit: usize,
		dependency_limit: usize,
		event_limit: usize,
	) -> Result<ChiefStoreSnapshot, StoreError> {
		page_limit(work_limit)?;
		page_limit(dependency_limit)?;
		page_limit(event_limit)?;
		self.run(move |connection| {
			let transaction = connection.transaction().map_err(sqlite_error)?;
			let count = |sql| -> Result<u64, StoreError> {
				let count: i64 = transaction.query_row(sql, [], |row| row.get(0)).map_err(sqlite_error)?;
				u64::try_from(count).map_err(|_| DatabaseError::Corrupt.into())
			};
			let work_count = count("SELECT count(*) FROM chief_work_items")?;
			let dependency_count = count("SELECT count(*) FROM chief_dependencies")?;
			let event_count = count("SELECT count(*) FROM chief_inbox_events WHERE disposition IS NULL")?;
			if work_count > work_limit as u64 || dependency_count > dependency_limit as u64 || event_count > event_limit as u64 {
				return Ok(ChiefStoreSnapshot::CapacityExceeded { work_items: work_count, dependencies: dependency_count, pending_events: event_count });
			}
			let work_items = transaction.prepare("SELECT * FROM chief_work_items ORDER BY created_at_micros, id").map_err(sqlite_error)?.query_map([], work_row).map_err(sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(sqlite_error)?;
			let dependencies = transaction.prepare("SELECT work_item_id, depends_on_id FROM chief_dependencies ORDER BY work_item_id, depends_on_id").map_err(sqlite_error)?.query_map([], |row| Ok(ChiefDependency { work_item_id: row.get(0)?, depends_on_id: row.get(1)? })).map_err(sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(sqlite_error)?;
			let pending_events = transaction.prepare("SELECT * FROM chief_inbox_events WHERE disposition IS NULL ORDER BY id").map_err(sqlite_error)?.query_map([], event_row).map_err(sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(sqlite_error)?;
            let managers=transaction.prepare("SELECT work_id FROM chief_managers").map_err(sqlite_error)?.query_map([],|row|row.get(0)).map_err(sqlite_error)?.collect::<Result<Vec<String>,_>>().map_err(sqlite_error)?;
            let workspaces=transaction.prepare("SELECT chief_id,name,directory FROM chief_workspaces ORDER BY chief_id").map_err(sqlite_error)?.query_map([],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(sqlite_error)?.collect::<Result<Vec<(String,String,String)>,_>>().map_err(sqlite_error)?;
            transaction.commit().map_err(sqlite_error)?;
            Ok(ChiefStoreSnapshot::Complete { managers,workspaces,work_items, dependencies, pending_events })
		}).await
	}

	pub async fn create_chief_work_item(
		&self,
		item: ChiefWorkItem,
	) -> Result<ChiefWorkItem, StoreError> {
		self.create_chief_work_record(item, false, None).await
	}

	/// Atomically create an executable manager and its optional workspace scope.
	pub async fn create_chief_manager(
		&self,
		item: ChiefWorkItem,
		workspace: Option<(String, String)>,
	) -> Result<ChiefWorkItem, StoreError> {
		if item.kind != ChiefWorkKind::Goal {
			return Err(StoreError::InvalidInput("manager must be a goal"));
		}
		self.create_chief_work_record(item, true, workspace).await
	}

	async fn create_chief_work_record(
		&self,
		item: ChiefWorkItem,
		manager: bool,
		workspace: Option<(String, String)>,
	) -> Result<ChiefWorkItem, StoreError> {
		bounded(&item.id, 512)?;
		bounded(&item.title, 1024)?;
		bounded(&item.instructions, 65536)?;
		if let Some((name, directory)) = &workspace {
			bounded(name, 256)?;
			bounded(directory, 4096)?;
		}
		if item.status != ChiefWorkStatus::Open
			|| item.codex_thread_id.is_some()
			|| item.dispatch_state != ChiefDispatchState::Idle
			|| item.active_turn_id.is_some()
			|| item.created_at_micros < 0
			|| item.updated_at_micros < item.created_at_micros
			|| item.next_check_at_micros.is_some_and(|time| time < 0)
		{
			return Err(StoreError::InvalidInput(
				"new Chief work must be open and unbound with valid timestamps",
			));
		}
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sqlite_error)?;
			if let Some(parent) = &item.parent_goal_id {
				let parent = read_work(&transaction, parent)?;
				if parent.kind != ChiefWorkKind::Goal {
					return Err(StoreError::InvalidInput("Chief parent must be a goal"));
				}
			}
			if work_exists(&transaction, &item.id)? {
				return Err(DatabaseError::AlreadyExists.into());
			}
			transaction
				.execute(
					"INSERT INTO chief_work_items (id, parent_goal_id, kind, title, instructions,
				codex_thread_id, status, next_check_at_micros, created_at_micros, updated_at_micros)
				VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'open', ?6, ?7, ?8)",
					params![
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
				.map_err(sqlite_error)?;
			if manager || item.parent_goal_id.is_none() {
				transaction
					.execute(
						"INSERT INTO chief_tool_versions(work_id,version) VALUES(?1,3)",
						[&item.id],
					)
					.map_err(sqlite_error)?;
			}

			if manager {
				transaction
					.execute("INSERT INTO chief_managers(work_id) VALUES(?1)", [&item.id])
					.map_err(sqlite_error)?;
				if let Some((name, directory)) = workspace {
					transaction
						.execute(
							"INSERT INTO chief_workspaces(chief_id,name,directory) VALUES(?1,?2,?3)",
							params![item.id, name, directory],
						)
						.map_err(sqlite_error)?;
				}
			}

			transaction.commit().map_err(sqlite_error)?;
			Ok(item)
		})
		.await
	}

	pub async fn list_chief_work_items(&self) -> Result<Vec<ChiefWorkItem>, StoreError> {
		self.run(|connection| {
			let mut statement = connection
				.prepare("SELECT * FROM chief_work_items ORDER BY created_at_micros, id")
				.map_err(sqlite_error)?;
			statement
				.query_map([], work_row)
				.map_err(sqlite_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(|error| sqlite_error(error).into())
		})
		.await
	}

	pub async fn get_chief_work_item(&self, id: String) -> Result<ChiefWorkItem, StoreError> {
		self.run(move |connection| read_work(connection, &id)).await
	}

	pub async fn set_chief_work_status(
		&self,
		id: String,
		status: ChiefWorkStatus,
		next_check_at_micros: Option<i64>,
	) -> Result<ChiefWorkItem, StoreError> {
		if next_check_at_micros.is_some_and(|time| time < 0)
			|| (status == ChiefWorkStatus::Resolved && next_check_at_micros.is_some())
		{
			return Err(StoreError::InvalidInput("invalid Chief next check"));
		}
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			read_work(&transaction, &id)?;
			transaction.execute("UPDATE chief_work_items SET status = ?2, next_check_at_micros = ?3, updated_at_micros = max(updated_at_micros, ?4) WHERE id = ?1",
				params![id, status.as_str(), next_check_at_micros, unix_micros()?]).map_err(sqlite_error)?;
			let item = read_work(&transaction, &id)?;
			transaction.commit().map_err(sqlite_error)?;
			Ok(item)
		}).await
	}

	pub async fn bind_chief_thread(
		&self,
		id: String,
		codex_thread_id: String,
	) -> Result<ChiefWorkItem, StoreError> {
		bounded(&codex_thread_id, 512)?;
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let item = read_work(&transaction, &id)?;
			if let Some(bound) = &item.codex_thread_id {
				return if bound == &codex_thread_id { Ok(item) } else { Err(DatabaseError::Conflict.into()) };
			}
			let used: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE codex_thread_id = ?1)", [&codex_thread_id], |row| row.get(0)).map_err(sqlite_error)?;
			if used { return Err(DatabaseError::Conflict.into()); }
			transaction.execute("UPDATE chief_work_items SET codex_thread_id = ?2, updated_at_micros = max(updated_at_micros, ?3) WHERE id = ?1",
				params![id, codex_thread_id, unix_micros()?]).map_err(sqlite_error)?;
			let updated = read_work(&transaction, &id)?;
			transaction.commit().map_err(sqlite_error)?;
			Ok(updated)
		}).await
	}

	pub async fn add_chief_dependency(
		&self,
		id: String,
		depends_on: String,
	) -> Result<(), StoreError> {
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			if !work_exists(&transaction, &id)? || !work_exists(&transaction, &depends_on)? { return Err(DatabaseError::NotFound.into()); }
			let cycle: bool = transaction.query_row("WITH RECURSIVE ancestors(id) AS (
				SELECT ?1 UNION SELECT depends_on_id FROM chief_dependencies JOIN ancestors ON work_item_id = ancestors.id)
				SELECT EXISTS(SELECT 1 FROM ancestors WHERE id = ?2)", params![depends_on, id], |row| row.get(0)).map_err(sqlite_error)?;
			if cycle { return Err(StoreError::InvalidInput("Chief dependency would create a cycle")); }
			transaction.execute("INSERT INTO chief_dependencies (work_item_id, depends_on_id) VALUES (?1, ?2) ON CONFLICT DO NOTHING", params![id, depends_on]).map_err(sqlite_error)?;
			transaction.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	pub async fn begin_chief_dispatch(&self, id: String) -> Result<ChiefWorkItem, StoreError> {
		self.begin_chief_dispatch_with_events(id, Vec::new()).await
	}

	/// Fence the first thread creation before contacting the provider.
	pub async fn begin_chief_thread_creation(
		&self,
		id: String,
	) -> Result<ChiefWorkItem, StoreError> {
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work = read_work(&transaction, &id)?;
			if work.dispatch_state != ChiefDispatchState::Idle || work.codex_thread_id.is_some() { return Err(DatabaseError::Conflict.into()); }
			transaction.execute("UPDATE chief_work_items SET dispatch_state = 'dispatching', updated_at_micros = max(updated_at_micros, ?2) WHERE id = ?1", params![id, unix_micros()?]).map_err(sqlite_error)?;
			let work = read_work(&transaction, &id)?;
			transaction.commit().map_err(sqlite_error)?;
			Ok(work)
		}).await
	}

	/// Bind one positively acknowledged first thread and release only its creation fence.
	pub async fn acknowledge_chief_thread_creation(
		&self,
		id: String,
		thread_id: String,
	) -> Result<ChiefWorkItem, StoreError> {
		bounded(&thread_id, 512)?;
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work = read_work(&transaction, &id)?;
			if work.dispatch_state != ChiefDispatchState::Dispatching || work.codex_thread_id.is_some() { return Err(DatabaseError::Conflict.into()); }
			transaction.execute("UPDATE chief_work_items SET codex_thread_id = ?2, dispatch_state = 'idle', updated_at_micros = max(updated_at_micros, ?3) WHERE id = ?1", params![id, thread_id, unix_micros()?]).map_err(sqlite_error)?;
			let work = read_work(&transaction, &id)?;
			transaction.commit().map_err(sqlite_error)?;
			Ok(work)
		}).await
	}

	/// Claim before the external effect. A restart never resets a dispatch claim.
	/// Delivered events remain pending until a separate explicit disposition.
	pub async fn begin_chief_dispatch_with_events(
		&self,
		id: String,
		event_ids: Vec<i64>,
	) -> Result<ChiefWorkItem, StoreError> {
		self.begin_chief_dispatch_with_input(id, event_ids, None).await
	}

	/// Atomically save a manager instruction with its dispatch claim, without a wake event.
	pub async fn begin_chief_dispatch_with_input(
		&self,
		id: String,
		event_ids: Vec<i64>,
		instruction: Option<String>,
	) -> Result<ChiefWorkItem, StoreError> {
		if let Some(text) = &instruction {
			bounded(text, 65536)?;
		}
		if event_ids.len() > 1000 {
			return Err(StoreError::InvalidInput("too many Chief dispatch events"));
		}
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work = read_work(&transaction, &id)?;
			if work.dispatch_state != ChiefDispatchState::Idle || work.codex_thread_id.is_none() || (crate::chief_permissions::pending(&transaction, &id)? || crate::chief_plugins::pending(&transaction, &id)?) {
				return Err(DatabaseError::Conflict.into());
			}
			capacity::cancel_pending(&transaction, &id)?;
			transaction.execute("UPDATE chief_work_items SET dispatch_state = 'dispatching', updated_at_micros = max(updated_at_micros, ?2) WHERE id = ?1", params![id, unix_micros()?]).map_err(sqlite_error)?;
			for event_id in event_ids {
				let changed = transaction.execute("WITH RECURSIVE owned(id) AS (
					SELECT ?2 UNION SELECT child.id FROM chief_work_items child JOIN owned ON child.parent_goal_id = owned.id WHERE owned.id=?2 OR NOT EXISTS(SELECT 1 FROM chief_managers WHERE work_id=owned.id))
					UPDATE chief_inbox_events SET delivery_work_item_id = ?2, delivered_turn_id = ''
					WHERE id = ?1 AND work_item_id IN (SELECT id FROM owned) AND disposition IS NULL
					AND (event_kind IN ('worker_turn_completed', 'automation_result', 'followup_due', 'user_message') OR (event_kind='async_question_answer' AND work_item_id=?2 AND delivered_turn_id IS NULL)) AND (work_item_id=?2 OR event_kind='worker_turn_completed' OR NOT EXISTS(SELECT 1 FROM chief_managers WHERE work_id=work_item_id)) AND (work_item_id<>?2 OR event_kind<>'worker_turn_completed')
					AND (delivered_turn_id IS NULL OR (delivery_work_item_id = ?2 AND delivered_turn_id != ''))", params![event_id, id]).map_err(sqlite_error)?;
				if changed != 1 { return Err(DatabaseError::Conflict.into()); }
                let event = read_event(&transaction, event_id)?;
                if event.event_kind == "user_message" { crate::chief_questions::retire_for_prompt(&transaction, &id, &event.payload)?; }
			}
			if work.kind == ChiefWorkKind::Task || transaction.query_row("SELECT EXISTS(SELECT 1 FROM chief_managers WHERE work_id=?1)",[&id],|row|row.get::<_,bool>(0)).map_err(sqlite_error)? {
				transaction.execute("UPDATE chief_work_items SET status = 'open', next_check_at_micros = NULL WHERE id = ?1", [&id]).map_err(sqlite_error)?;
			}
			if let Some(text)=instruction {
				let now=unix_micros()?;
				let previous:i64=transaction.query_row("SELECT coalesce(max(id),0) FROM chief_inbox_events WHERE work_item_id=?1",[&id],|row|row.get(0)).map_err(sqlite_error)?;
				let source=serde_json::json!(["work_instruction",id,previous]).to_string();
				let payload=serde_json::json!({"text":text,"source":"manager"}).to_string();
				if payload.len()>65536 {return Err(StoreError::InvalidInput("instruction exceeds saved event bound"));}
				transaction.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,?2,'work_instruction',?3,?4,?2,'')",params![source,id,payload,now]).map_err(sqlite_error)?;
			}

			let work = read_work(&transaction, &id)?;
			transaction.commit().map_err(sqlite_error)?;
			Ok(work)
		}).await
	}

	/// Fence a steering attempt before the provider effect. Pending attempts never queue.
	pub async fn begin_chief_steer(
		&self,
		id: String,
		turn: String,
		key: String,
		payload: String,
	) -> Result<i64, StoreError> {
		bounded(&payload, 65536)?;
		bounded(&key, 512)?;
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work = read_work(&tx, &id)?;
			if work.dispatch_state != ChiefDispatchState::Running || work.active_turn_id.as_deref() != Some(&turn) {
				return Err(DatabaseError::Conflict.into());
			}
			crate::chief_task_references::validate_references(&tx, &payload)?;
			let mut value: serde_json::Value = serde_json::from_str(&payload)
				.map_err(|_| StoreError::InvalidInput("invalid steering input"))?;
			let fields = value.as_object_mut().ok_or(StoreError::InvalidInput("invalid steering input"))?;
			fields.insert("threadId".into(), serde_json::json!(work.codex_thread_id));
			let payload = value.to_string();
			bounded(&payload, 65536)?;
			let source = serde_json::json!(["user_steer", id, key]).to_string();
			tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,?2,'steer_pending',?3,?4,?2,?5)",params![source,id,payload,unix_micros()?,turn]).map_err(sqlite_error)?;
			let event = tx.last_insert_rowid();
			tx.commit().map_err(sqlite_error)?;
			Ok(event)
		}).await
	}

	/// Publish only confirmed steering input as a delivered user message.
	pub async fn finish_chief_steer(&self, event: i64, accepted: bool) -> Result<(), StoreError> {
		self.run(move |connection| {
			let tx = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sqlite_error)?;
			steer::finish(&tx, event, accepted)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		})
		.await
	}

	pub async fn acknowledge_chief_dispatch(
		&self,
		id: String,
		turn_id: String,
	) -> Result<ChiefWorkItem, StoreError> {
		self.acknowledge_chief_dispatch_with_execution(id, turn_id, None).await
	}

	/// Bind requested execution settings in the same transaction as the native acknowledgment.
	pub async fn acknowledge_chief_dispatch_with_execution(
		&self,
		id: String,
		turn_id: String,
		execution: Option<crate::ChiefTurnExecution>,
	) -> Result<ChiefWorkItem, StoreError> {
		bounded(&turn_id, 512)?;
		if let Some(execution) = &execution {
			execution.validate()?;
		}
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work = read_work(&transaction, &id)?;
			if work.dispatch_state != ChiefDispatchState::Dispatching { return Err(DatabaseError::Conflict.into()); }
			if let Some(execution) = &execution {
				let thread = work.codex_thread_id.as_deref().ok_or(DatabaseError::Conflict)?;
				crate::chief_turn_execution::record(&transaction, &id, thread, &turn_id, execution)?;
			}
			transaction.execute("UPDATE chief_work_items SET dispatch_state = 'running', active_turn_id = ?2, updated_at_micros = max(updated_at_micros, ?3) WHERE id = ?1", params![id, turn_id, unix_micros()?]).map_err(sqlite_error)?;
			transaction.execute("UPDATE chief_usage SET turn_id=?2,baseline_input_tokens=json_extract(usage_json,'$.input_tokens'),baseline_output_tokens=json_extract(usage_json,'$.output_tokens'),turn_input_tokens=NULL,turn_output_tokens=NULL WHERE work_id=?1 AND thread_id=?3",params![id,turn_id,work.codex_thread_id]).map_err(sqlite_error)?;
			transaction.execute("UPDATE chief_inbox_events SET delivered_turn_id = ?2 WHERE delivery_work_item_id = ?1 AND delivered_turn_id = '' AND disposition IS NULL", params![id, turn_id]).map_err(sqlite_error)?;
			transaction.execute("UPDATE chief_inbox_events SET disposition='resolved',disposition_note='Instruction accepted by the provider; completion is tracked separately.',disposed_at_micros=max(created_at_micros,?3) WHERE delivery_work_item_id=?1 AND delivered_turn_id=?2 AND event_kind='work_instruction' AND disposition IS NULL",params![id,turn_id,unix_micros()?]).map_err(sqlite_error)?;

			transaction.execute("UPDATE chief_capacity_retries SET state='submitted',retry_turn_id=?2 WHERE work_item_id=?1 AND state='claimed'",params![id,turn_id]).map_err(sqlite_error)?;
			let work = read_work(&transaction, &id)?;
			transaction.commit().map_err(sqlite_error)?;
			Ok(work)
		}).await
	}

	pub async fn complete_chief_turn(
		&self,
		id: String,
		turn_id: String,
	) -> Result<ChiefWorkItem, StoreError> {
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work = read_work(&transaction, &id)?;
			if work.dispatch_state != ChiefDispatchState::Running || work.active_turn_id.as_deref() != Some(turn_id.as_str()) {
				return Err(DatabaseError::Conflict.into());
			}
			transaction.execute("UPDATE chief_work_items SET dispatch_state = 'idle', active_turn_id = NULL, updated_at_micros = max(updated_at_micros, ?2) WHERE id = ?1", params![id, unix_micros()?]).map_err(sqlite_error)?;
			let work = read_work(&transaction, &id)?;
			transaction.commit().map_err(sqlite_error)?;
			Ok(work)
		}).await
	}

	/// Record the exact terminal event and release its running dispatch atomically.
	pub async fn complete_chief_turn_with_event(
		&self,
		id: String,
		turn_id: String,
		input: EnqueueChiefEvent,
	) -> Result<ChiefInboxEvent, StoreError> {
		bounded(&input.source_event_id, 2048)?;
		bounded(&input.event_kind, 128)?;
		if input.payload.len() > 65536 || input.work_item_id != id {
			return Err(StoreError::InvalidInput("invalid Chief terminal event"));
		}
		let user_input_handled = ["chief_turn_completed", "worker_turn_completed"]
			.contains(&input.event_kind.as_str())
			&& serde_json::from_str::<serde_json::Value>(&input.payload).is_ok_and(|payload| {
				payload.pointer("/terminal/turn/status").and_then(serde_json::Value::as_str)
					== Some("completed")
			});
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work = read_work(&transaction, &id)?;
			if work.dispatch_state != ChiefDispatchState::Running || work.active_turn_id.as_deref() != Some(turn_id.as_str()) {
				return Err(DatabaseError::Conflict.into());
			}
			let mut input = input;
            let mut payload: serde_json::Value = serde_json::from_str(&input.payload).unwrap_or_default();
            let eligible = work.status == ChiefWorkStatus::Open
                && matches!(input.event_kind.as_str(), "chief_turn_completed" | "worker_turn_completed")
                && payload.pointer("/terminal/turn/status").and_then(serde_json::Value::as_str)==Some("failed")
                && payload.pointer("/terminal/turn/error/codexErrorInfo").and_then(serde_json::Value::as_str)==Some("serverOverloaded")
                && payload.pointer("/threadReadback/capacityRetryEligible")==Some(&serde_json::json!(true));
            let mut retry = if eligible { capacity::next_retry(&transaction,&id,&turn_id,unix_micros()?)? } else { None };
            if eligible && retry.is_none() {
                payload["capacityRetry"]=serde_json::json!({"exhausted":true,"attempt":3});
                let encoded=payload.to_string();
                if encoded.len()<=65536 {input.payload=encoded;}
            }
            if let Some((attempt,due))=retry {
                payload["capacityRetry"]=serde_json::json!({"attempt":attempt,"dueAtMicros":due});
                let encoded=payload.to_string();
                if encoded.len()<=65536 {
                    input.event_kind="capacity_retry".into();
                    input.payload=encoded;
                } else { retry=None; }
            }
			let previous = transaction.query_row("SELECT * FROM chief_inbox_events WHERE source_event_id = ?1", [&input.source_event_id], event_row).optional().map_err(sqlite_error)?;
			let event = if let Some(event) = previous {
				if event.work_item_id != input.work_item_id || event.event_kind != input.event_kind || event.payload != input.payload {
					return Err(StoreError::IdempotencyConflict);
				}
				event
			} else {
				transaction.execute("INSERT INTO chief_inbox_events (source_event_id, work_item_id, event_kind, payload, created_at_micros) VALUES (?1, ?2, ?3, ?4, ?5)",
					params![input.source_event_id, input.work_item_id, input.event_kind, input.payload, unix_micros()?]).map_err(sqlite_error)?;
				read_event(&transaction, transaction.last_insert_rowid())?
			};
			transaction.execute("UPDATE chief_work_items SET dispatch_state = 'idle', active_turn_id = NULL, updated_at_micros = max(updated_at_micros, ?2) WHERE id = ?1", params![id, unix_micros()?]).map_err(sqlite_error)?;
			let event = if event.event_kind == "chief_turn_completed" && event.disposition.is_none() {
				transaction.execute("UPDATE chief_inbox_events SET disposition = 'resolved', disposition_note = 'Chief turn receipt recorded; work judgment is unchanged.', disposed_at_micros = max(created_at_micros, ?2) WHERE id = ?1", params![event.id, unix_micros()?]).map_err(sqlite_error)?;
				read_event(&transaction, event.id)?
			} else { event };
			if user_input_handled {
				transaction.execute("UPDATE chief_inbox_events SET disposition = 'resolved', disposition_note = 'User input handled by completed Chief turn; work judgment is unchanged.', disposed_at_micros = max(created_at_micros, ?3) WHERE disposition IS NULL AND event_kind IN ('user_message', 'async_question_answer') AND delivery_work_item_id = ?1 AND delivered_turn_id = ?2", params![id, turn_id, unix_micros()?]).map_err(sqlite_error)?;
			}
            if let Some((attempt,due))=retry {
                transaction.execute("INSERT INTO chief_capacity_retries (event_id,work_item_id,failed_turn_id,attempt,due_at_micros,state) VALUES (?1,?2,?3,?4,?5,'pending')",params![event.id,id,turn_id,attempt,due]).map_err(sqlite_error)?;
            }
			transaction.commit().map_err(sqlite_error)?;
			Ok(event)
		}).await
	}

	/// A caller with positive external turn evidence can reconcile an unknown dispatch.
	/// This never resets a claim to idle or authorizes a new external dispatch.
	pub async fn reconcile_chief_dispatch(
		&self,
		id: String,
		turn_id: String,
	) -> Result<ChiefWorkItem, StoreError> {
		bounded(&turn_id, 512)?;
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work = read_work(&transaction, &id)?;
			if !matches!(work.dispatch_state, ChiefDispatchState::Dispatching | ChiefDispatchState::Unknown) { return Err(DatabaseError::Conflict.into()); }
			if work.active_turn_id.as_deref().is_some_and(|active| active != turn_id) {
				return Err(DatabaseError::Conflict.into());
			}
			transaction.execute("UPDATE chief_work_items SET dispatch_state = 'running', active_turn_id = ?2, updated_at_micros = max(updated_at_micros, ?3) WHERE id = ?1", params![id, turn_id, unix_micros()?]).map_err(sqlite_error)?;
			transaction.execute("UPDATE chief_inbox_events SET delivered_turn_id = ?2 WHERE delivery_work_item_id = ?1 AND delivered_turn_id = '' AND disposition IS NULL", params![id, turn_id]).map_err(sqlite_error)?;
			transaction.execute("UPDATE chief_capacity_retries SET state='submitted',retry_turn_id=?2 WHERE work_item_id=?1 AND state='claimed'",params![id,turn_id]).map_err(sqlite_error)?;
			let work = read_work(&transaction, &id)?;
			transaction.commit().map_err(sqlite_error)?;
			Ok(work)
		}).await
	}

	/// Preserve an ambiguous external effect and any acknowledged turn identity.
	/// This state has no automatic retry transition.
	pub async fn mark_chief_dispatch_unknown(
		&self,
		id: String,
	) -> Result<ChiefWorkItem, StoreError> {
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work = read_work(&transaction, &id)?;
			if !matches!(work.dispatch_state, ChiefDispatchState::Dispatching | ChiefDispatchState::Running) { return Err(DatabaseError::Conflict.into()); }
			transaction.execute("UPDATE chief_work_items SET dispatch_state = 'unknown', updated_at_micros = max(updated_at_micros, ?2) WHERE id = ?1", params![id, unix_micros()?]).map_err(sqlite_error)?;
			let work = read_work(&transaction, &id)?;
			transaction.commit().map_err(sqlite_error)?;
			Ok(work)
		}).await
	}

	pub async fn list_chief_dependencies(&self) -> Result<Vec<ChiefDependency>, StoreError> {
		self.run(|connection| {
			let mut statement = connection.prepare("SELECT work_item_id, depends_on_id FROM chief_dependencies ORDER BY work_item_id, depends_on_id").map_err(sqlite_error)?;
			statement.query_map([], |row| Ok(ChiefDependency { work_item_id: row.get(0)?, depends_on_id: row.get(1)? })).map_err(sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(|error| sqlite_error(error).into())
		}).await
	}

	pub async fn enqueue_chief_event(
		&self,
		input: EnqueueChiefEvent,
	) -> Result<ChiefInboxEvent, StoreError> {
		self.insert_chief_event(input, false).await
	}

	/// Save a provider observation without creating a model wake or an unresolved obligation.
	pub async fn record_chief_observation(
		&self,
		input: EnqueueChiefEvent,
	) -> Result<ChiefInboxEvent, StoreError> {
		if !matches!(
			input.event_kind.as_str(),
			"assistant_message" | "token_usage" | "context_compacted"
		) || !serde_json::from_str::<serde_json::Value>(&input.payload)
			.is_ok_and(|value| value.is_object())
		{
			return Err(StoreError::InvalidInput("invalid Chief observation"));
		}
		self.insert_chief_event(input, true).await
	}

	/// Read the last observed usage for one exact work turn, including after restart.
	pub async fn read_chief_usage_observation(
		&self,
		work_id: String,
		thread_id: String,
		turn_id: String,
	) -> Result<Option<ChiefInboxEvent>, StoreError> {
		self.run(move |connection| {
			connection.query_row("SELECT * FROM chief_inbox_events WHERE work_item_id = ?1 AND event_kind = 'token_usage' AND json_extract(payload, '$.threadId') = ?2 AND json_extract(payload, '$.turnId') = ?3 ORDER BY id DESC LIMIT 1", params![work_id, thread_id, turn_id], event_row)
				.optional().map_err(|error| sqlite_error(error).into())
		}).await
	}

	async fn insert_chief_event(
		&self,
		input: EnqueueChiefEvent,
		observation: bool,
	) -> Result<ChiefInboxEvent, StoreError> {
		bounded(&input.source_event_id, 2048)?;
		bounded(&input.event_kind, 128)?;
		if input.payload.len() > 65536 {
			return Err(StoreError::InvalidInput("Chief event payload is too large"));
		}
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let previous = transaction.query_row("SELECT * FROM chief_inbox_events WHERE source_event_id = ?1", [&input.source_event_id], event_row).optional().map_err(sqlite_error)?;
			if let Some(event) = previous {
				return if event.work_item_id == input.work_item_id && event.event_kind == input.event_kind && event.payload == input.payload {
					Ok(event)
				} else { Err(StoreError::IdempotencyConflict) };
			}
			if !work_exists(&transaction, &input.work_item_id)? { return Err(DatabaseError::NotFound.into()); }
			if input.event_kind == "user_message" { crate::chief_task_references::validate_references(&transaction, &input.payload)?; }
            if input.event_kind == "user_message" && transaction.query_row("SELECT EXISTS(SELECT 1 FROM chief_misalignment m JOIN chief_work_items w ON w.id=m.work_id AND w.codex_thread_id=m.thread_id WHERE m.work_id=?1)",[&input.work_item_id],|row|row.get::<_,bool>(0)).map_err(sqlite_error)? { return Err(StoreError::InvalidInput("conversation paused for provider findings")); }

			let now = unix_micros()?;
			transaction.execute("INSERT INTO chief_inbox_events (source_event_id, work_item_id, event_kind, payload, created_at_micros, disposition, disposition_note, disposed_at_micros) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
				params![input.source_event_id, input.work_item_id, input.event_kind, input.payload, now,
					observation.then_some("resolved"), observation.then_some("Provider observation recorded; work judgment unchanged."), observation.then_some(now)]).map_err(sqlite_error)?;
			let event = read_event(&transaction, transaction.last_insert_rowid())?;
            if input.event_kind == "user_message" { crate::chief_questions::retire_for_prompt(&transaction, &input.work_item_id, &input.payload)?; }
			transaction.commit().map_err(sqlite_error)?;
			Ok(event)
		}).await
	}

	/// Record one active connection failure. Repeated probes do not duplicate it.
	pub async fn record_chief_connection_failure(
		&self,
		root: String,
		detail: String,
	) -> Result<(), StoreError> {
		bounded(&root, 512)?;
		bounded(&detail, 65536)?;
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let payload = serde_json::json!({"recovery":detail}).to_string();
            let pending: Option<(i64, String)> = tx.query_row("SELECT id,payload FROM chief_inbox_events WHERE work_item_id=?1 AND event_kind='reconnection_needs_attention' AND disposition IS NULL ORDER BY id DESC LIMIT 1", [&root], |row| Ok((row.get(0)?,row.get(1)?))).optional().map_err(sqlite_error)?;
            if pending.as_ref().is_some_and(|(_, previous)| previous == &payload) { return Ok(()); }
            if let Some((id, _)) = pending {
                tx.execute("UPDATE chief_inbox_events SET disposition='resolved', disposition_note='Superseded by a newer connection diagnostic; connectivity is not yet restored.', disposed_at_micros=max(created_at_micros,?2) WHERE id=?1", params![id,unix_micros()?]).map_err(sqlite_error)?;
            }
            {
				let now = unix_micros()?;
				let previous: i64 = tx.query_row("SELECT coalesce(max(id),0) FROM chief_inbox_events WHERE work_item_id=?1", [&root], |row| row.get(0)).map_err(sqlite_error)?;
				let source = serde_json::json!(["chief_connection", root, previous]).to_string();
				let payload = serde_json::json!({"recovery":detail}).to_string();
				tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES(?1,?2,'reconnection_needs_attention',?3,?4)", params![source,root,payload,now]).map_err(sqlite_error)?;
			}
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// Close connection errors after an attested connection, without changing work judgment.
	pub async fn resolve_chief_connection_failure(&self, root: String) -> Result<(), StoreError> {
		self.run(move |connection| {
			connection.execute("UPDATE chief_inbox_events SET disposition='resolved',disposition_note='The Chief connection was restored.',disposed_at_micros=max(created_at_micros,?2) WHERE work_item_id=?1 AND event_kind='reconnection_needs_attention' AND disposition IS NULL",params![root,unix_micros()?]).map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// Keep one current delivery error, with separate records for later recurrences.
	pub async fn record_chief_delivery_failure(
		&self,
		root: String,
		detail: String,
	) -> Result<(), StoreError> {
		self.record_chief_delivery_notice(root, detail, "wake_failed").await
	}

	/// Record an attested external writer without treating it as an execution failure.
	pub async fn record_chief_thread_in_use(
		&self,
		root: String,
		detail: String,
	) -> Result<(), StoreError> {
		self.record_chief_delivery_notice(root, detail, "thread_in_use_needs_attention").await
	}

	async fn record_chief_delivery_notice(
		&self,
		root: String,
		detail: String,
		kind: &'static str,
	) -> Result<(), StoreError> {
		bounded(&root, 512)?;
		bounded(&detail, 2048)?;
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let payload = serde_json::json!({"recovery":detail}).to_string();
			let same: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events WHERE work_item_id=?1 AND event_kind=?3 AND payload=?2 AND disposition IS NULL)",params![root,payload,kind],|row|row.get(0)).map_err(sqlite_error)?;
			if !same {
				tx.execute("UPDATE chief_inbox_events SET disposition='resolved',disposition_note='Superseded by the current delivery diagnostic.',disposed_at_micros=max(created_at_micros,?2) WHERE work_item_id=?1 AND event_kind IN ('wake_failed','followup_processing_failed','thread_in_use_needs_attention') AND disposition IS NULL",params![root,unix_micros()?]).map_err(sqlite_error)?;
				let previous:i64 = tx.query_row("SELECT coalesce(max(id),0) FROM chief_inbox_events WHERE work_item_id=?1",[&root],|row|row.get(0)).map_err(sqlite_error)?;
				let source = serde_json::json!(["chief_delivery",root,previous]).to_string();
				tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES(?1,?2,?5,?3,?4)",params![source,root,payload,unix_micros()?,kind]).map_err(sqlite_error)?;
			}
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// Keep unsent input visible in history, but require a new user send after an ownership
	/// conflict.
	pub async fn hold_chief_unsent_input(&self, work: String) -> Result<(), StoreError> {
		self.run(move |connection| {
            connection.execute("UPDATE chief_inbox_events SET disposition='user_decision', disposition_note='Not sent: conversation was in use elsewhere. Send again when ready.', disposed_at_micros=max(created_at_micros,?2) WHERE work_item_id=?1 AND event_kind='user_message' AND disposition IS NULL AND delivered_turn_id IS NULL", params![work,unix_micros()?]).map_err(sqlite_error)?;
            Ok(())
        }).await
	}

	/// Successful delivery processing clears only delivery diagnostics, never work events.
	pub async fn resolve_chief_delivery_failure(&self, root: String) -> Result<(), StoreError> {
		self.run(move |connection| {
			connection.execute("UPDATE chief_inbox_events SET disposition='resolved',disposition_note='Chief delivery processing recovered.',disposed_at_micros=max(created_at_micros,?2) WHERE work_item_id=?1 AND event_kind IN ('wake_failed','followup_processing_failed','thread_in_use_needs_attention') AND disposition IS NULL",params![root,unix_micros()?]).map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// Read without claiming or acknowledging. Failed processing leaves every event pending.
	pub async fn list_undelivered_chief_events(
		&self,
		limit: usize,
	) -> Result<Vec<ChiefInboxEvent>, StoreError> {
		let limit = page_limit(limit)?;
		self.run(move |connection| {
			connection.prepare("SELECT * FROM chief_inbox_events WHERE disposition IS NULL AND delivered_turn_id IS NULL AND event_kind IN ('worker_turn_completed', 'automation_result', 'followup_due', 'user_message') ORDER BY id LIMIT ?1").map_err(sqlite_error)?.query_map([limit], event_row).map_err(sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(|error| sqlite_error(error).into())
		}).await
	}

	/// Select fresh triggers first, then unresolved evidence previously delivered to this Chief.
	/// Reading this batch does not authorize a wake without at least one fresh trigger.
	pub async fn list_chief_wake_events(
		&self,
		chief_id: String,
		limit: usize,
	) -> Result<Vec<ChiefInboxEvent>, StoreError> {
		let limit = page_limit(limit)?;
		self.run(move |connection| {
			connection.prepare("WITH RECURSIVE owned(id) AS (
				SELECT ?1 UNION SELECT child.id FROM chief_work_items child JOIN owned ON child.parent_goal_id = owned.id WHERE owned.id = ?1 OR NOT EXISTS(SELECT 1 FROM chief_managers WHERE work_id=owned.id))
				SELECT * FROM chief_inbox_events WHERE work_item_id IN (SELECT id FROM owned)
				AND disposition IS NULL AND event_kind IN ('worker_turn_completed', 'automation_result', 'followup_due', 'user_message')
                AND (work_item_id <> ?1 OR event_kind <> 'worker_turn_completed') AND (work_item_id=?1 OR event_kind='worker_turn_completed' OR NOT EXISTS(SELECT 1 FROM chief_managers WHERE work_id=work_item_id))
				AND (delivered_turn_id IS NULL OR (delivery_work_item_id = ?1 AND delivered_turn_id != ''))
				ORDER BY delivered_turn_id IS NOT NULL, id LIMIT ?2")
				.map_err(sqlite_error)?.query_map(params![chief_id, limit], event_row)
				.map_err(sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(|error| sqlite_error(error).into())
		}).await
	}

	/// Read this exact turn's undisposed delivery receipts before applying the page bound.
	pub async fn list_chief_events_for_turn(
		&self,
		turn_id: String,
		limit: usize,
	) -> Result<Vec<ChiefInboxEvent>, StoreError> {
		bounded(&turn_id, 512)?;
		let limit = page_limit(limit)?;
		self.run(move |connection| {
			connection.prepare("SELECT * FROM chief_inbox_events WHERE disposition IS NULL AND delivered_turn_id = ?1 ORDER BY id LIMIT ?2").map_err(sqlite_error)?.query_map(params![turn_id, limit], event_row).map_err(sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(|error| sqlite_error(error).into())
		}).await
	}

	/// Read without claiming or acknowledging. Failed processing leaves every event pending.
	pub async fn list_pending_chief_events(
		&self,
		limit: usize,
	) -> Result<Vec<ChiefInboxEvent>, StoreError> {
		let limit = page_limit(limit)?;
		self.run(move |connection| {
			let mut statement = connection
				.prepare(
					"SELECT * FROM chief_inbox_events WHERE disposition IS NULL ORDER BY id LIMIT ?1",
				)
				.map_err(sqlite_error)?;
			statement
				.query_map([limit], event_row)
				.map_err(sqlite_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(|error| sqlite_error(error).into())
		})
		.await
	}

	/// Acknowledge one response delivered to a current-connection provider request.
	/// The runtime must verify that the event is in its current connection request map.
	/// This receipt does not make a work judgment or change the next check time.
	pub async fn acknowledge_chief_request_event(
		&self,
		event_id: i64,
	) -> Result<ChiefInboxEvent, StoreError> {
		self.finish_chief_request_event(
			event_id,
			"Response delivered to the current provider request; work judgment is unchanged.",
		)
		.await
	}

	/// Record the provider resolution without claiming that this client sent a response.
	pub async fn resolve_chief_request_event(
		&self,
		event_id: i64,
	) -> Result<ChiefInboxEvent, StoreError> {
		self.finish_chief_request_event(event_id, "Provider resolved the current request; no local response was sent and work judgment is unchanged.").await
	}

	async fn finish_chief_request_event(
		&self,
		event_id: i64,
		note: &'static str,
	) -> Result<ChiefInboxEvent, StoreError> {
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let event = read_event(&transaction, event_id)?;
			if !matches!(event.event_kind.as_str(), "permission_pending" | "user_input_pending" | "server_request_pending") {
				return Err(StoreError::InvalidInput("event is not a Chief provider request"));
			}
			if event.disposition.is_some() { return Err(DatabaseError::Conflict.into()); }
			transaction.execute("UPDATE chief_inbox_events SET disposition = 'resolved', disposition_note = ?3, disposed_at_micros = max(created_at_micros, ?2) WHERE id = ?1 AND disposition IS NULL", params![event_id, unix_micros()?, note]).map_err(sqlite_error)?;
			let updated = read_event(&transaction, event_id)?;
			transaction.commit().map_err(sqlite_error)?;
			Ok(updated)
		}).await
	}

	/// Resolve an idle work decision from a current Chief turn's exact user message.
	/// Original decision evidence remains immutable; the new receipt links the reply.
	pub async fn resolve_chief_user_decision(
		&self,
		chief_id: String,
		work_id: String,
		turn_id: String,
		user_event_id: i64,
		note: String,
	) -> Result<(), StoreError> {
		bounded(&note, 65536)?;
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let source = read_event(&transaction, user_event_id)?;
			let active: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE id = ?1 AND (parent_goal_id IS NULL OR EXISTS(SELECT 1 FROM chief_managers WHERE work_id=?1)) AND active_turn_id = ?2 AND dispatch_state = 'running')",params![chief_id,turn_id],|row|row.get(0)).map_err(sqlite_error)?;
			let descendant: bool = transaction.query_row("WITH RECURSIVE lineage(id,parent_goal_id) AS (SELECT id,parent_goal_id FROM chief_work_items WHERE id = ?1 UNION SELECT work.id,work.parent_goal_id FROM chief_work_items work JOIN lineage ON work.id = lineage.parent_goal_id) SELECT EXISTS(SELECT 1 FROM lineage WHERE id = ?2)",params![work_id,chief_id],|row|row.get(0)).map_err(sqlite_error)?;
			if !active || !descendant || source.work_item_id != chief_id || !matches!(source.event_kind.as_str(), "user_message" | "async_question_answer") || source.delivered_turn_id.as_deref() != Some(&turn_id) || source.disposition.is_some() {
				return Err(StoreError::InvalidInput("decision requires a current delivered user reply"));
			}
			let now = unix_micros()?;
			let changed = transaction.execute("UPDATE chief_work_items SET status = 'resolved', next_check_at_micros = NULL, updated_at_micros = max(updated_at_micros, ?2) WHERE id = ?1 AND status = 'user_decision' AND dispatch_state = 'idle'",params![work_id,now]).map_err(sqlite_error)?;
			if changed != 1 {return Err(DatabaseError::Conflict.into());}
			let source_id = serde_json::json!(["user_decision_resolved",work_id,user_event_id]).to_string();
			let payload = serde_json::json!({"userEventId":user_event_id,"chiefTurnId":turn_id,"summary":note}).to_string();
			transaction.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES (?1,?2,'user_decision_resolved',?3,?4,'resolved',?5,?4)",params![source_id,work_id,payload,now,note]).map_err(sqlite_error)?;
			transaction.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// Record a model's explicit goal judgment with current, related evidence.
	pub async fn resolve_chief_goal(
		&self,
		chief_id: String,
		goal_id: String,
		turn_id: String,
		evidence_event_id: i64,
		note: String,
	) -> Result<(), StoreError> {
		bounded(&note, 65536)?;
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let event = read_event(&transaction,evidence_event_id)?;
			let current:bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE id = ?1 AND (parent_goal_id IS NULL OR EXISTS(SELECT 1 FROM chief_managers WHERE work_id=?1)) AND active_turn_id = ?2 AND dispatch_state = 'running')",params![chief_id,turn_id],|row|row.get(0)).map_err(sqlite_error)?;
			let related:bool = transaction.query_row("WITH RECURSIVE family(id) AS (SELECT id FROM chief_work_items WHERE id = ?1 UNION SELECT work.id FROM chief_work_items work JOIN family ON work.parent_goal_id = family.id) SELECT EXISTS(SELECT 1 FROM family WHERE id = ?2)",params![goal_id,event.work_item_id],|row|row.get(0)).map_err(sqlite_error)?;
			let user_input = matches!(event.event_kind.as_str(), "user_message" | "async_question_answer") && event.work_item_id == chief_id;
			let result = related && matches!(event.event_kind.as_str(),"worker_turn_completed"|"automation_result"|"followup_due");
			if !current || event.delivered_turn_id.as_deref() != Some(&turn_id) || (!user_input && !result) {return Err(StoreError::InvalidInput("goal resolution requires current related evidence"));}
			let now = unix_micros()?;
			let changed = transaction.execute("UPDATE chief_work_items SET status = 'resolved', next_check_at_micros = NULL, updated_at_micros = max(updated_at_micros,?3) WHERE id = ?1 AND kind = 'goal' AND parent_goal_id = ?2 AND dispatch_state = 'idle' AND status <> 'resolved'",params![goal_id,chief_id,now]).map_err(sqlite_error)?;
			if changed != 1 {return Err(DatabaseError::Conflict.into());}
			let source = serde_json::json!(["goal_resolved",goal_id,evidence_event_id]).to_string();
			let payload = serde_json::json!({"evidenceEventId":evidence_event_id,"chiefTurnId":turn_id,"summary":note}).to_string();
			transaction.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES (?1,?2,'goal_resolved',?3,?4,'resolved',?5,?4)",params![source,goal_id,payload,now,note]).map_err(sqlite_error)?;
			transaction.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// Commit one explicit decision and the next wake time in the same transaction.
	pub async fn dispose_chief_event(
		&self,
		id: i64,
		disposition: ChiefDisposition,
		note: String,
		next_check_at_micros: Option<i64>,
	) -> Result<ChiefInboxEvent, StoreError> {
		bounded(&note, 65536)?;
		if next_check_at_micros.is_some_and(|time| time < 0)
			|| (disposition == ChiefDisposition::Resolved && next_check_at_micros.is_some())
		{
			return Err(StoreError::InvalidInput("invalid Chief next check"));
		}
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let event = read_event(&transaction, id)?;
			if event.disposition.is_some() { return Err(DatabaseError::Conflict.into()); }
			let now = unix_micros()?.max(event.created_at_micros);
			transaction.execute("UPDATE chief_inbox_events SET disposition = ?2, disposition_note = ?3, disposed_at_micros = ?4 WHERE id = ?1 AND disposition IS NULL",
				params![id, disposition.as_str(), note, now]).map_err(sqlite_error)?;
			transaction.execute("UPDATE chief_work_items SET status = ?2, next_check_at_micros = ?3, updated_at_micros = max(updated_at_micros, ?4) WHERE id = ?1",
				params![event.work_item_id, disposition.as_str(), next_check_at_micros, now]).map_err(sqlite_error)?;
			let updated = read_event(&transaction, id)?;
			transaction.commit().map_err(sqlite_error)?;
			Ok(updated)
		}).await
	}

	pub async fn list_due_chief_work_items(
		&self,
		now_micros: i64,
		limit: usize,
	) -> Result<Vec<ChiefWorkItem>, StoreError> {
		let limit = page_limit(limit)?;
		self.run(move |connection| {
			let mut statement = connection.prepare("SELECT * FROM chief_work_items WHERE next_check_at_micros <= ?1 ORDER BY next_check_at_micros, id LIMIT ?2").map_err(sqlite_error)?;
			statement.query_map(params![now_micros, limit], work_row).map_err(sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(|error| sqlite_error(error).into())
		}).await
	}

	/// Select due work that has no durable notification for this exact due timestamp.
	/// The exclusion precedes the limit so old due work cannot starve later work.
	pub async fn list_unnotified_due_chief_work_items(
		&self,
		now_micros: i64,
		limit: usize,
	) -> Result<Vec<ChiefWorkItem>, StoreError> {
		let limit = page_limit(limit)?;
		self.run(move |connection| {
			connection.prepare("SELECT work.* FROM chief_work_items AS work WHERE next_check_at_micros <= ?1 AND NOT EXISTS (SELECT 1 FROM chief_inbox_events AS event WHERE event.source_event_id = json_array('followup_due', work.id, work.next_check_at_micros)) ORDER BY next_check_at_micros, id LIMIT ?2").map_err(sqlite_error)?.query_map(params![now_micros, limit], work_row).map_err(sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(|error| sqlite_error(error).into())
		}).await
	}
}

fn bounded(value: &str, max: usize) -> Result<(), StoreError> {
	if value.trim().is_empty() || value.len() > max {
		Err(StoreError::InvalidInput("Chief text is empty or too large"))
	} else {
		Ok(())
	}
}

fn page_limit(limit: usize) -> Result<i64, StoreError> {
	if (1..=1000).contains(&limit) {
		Ok(limit as i64)
	} else {
		Err(StoreError::InvalidInput("Chief page size must be between 1 and 1000"))
	}
}

fn work_exists(connection: &Connection, id: &str) -> Result<bool, StoreError> {
	connection
		.query_row("SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE id = ?1)", [id], |row| {
			row.get(0)
		})
		.map_err(|error| sqlite_error(error).into())
}

fn read_work(connection: &Connection, id: &str) -> Result<ChiefWorkItem, StoreError> {
	connection
		.query_row("SELECT * FROM chief_work_items WHERE id = ?1", [id], work_row)
		.optional()
		.map_err(sqlite_error)?
		.ok_or_else(|| DatabaseError::NotFound.into())
}

fn read_event(connection: &Connection, id: i64) -> Result<ChiefInboxEvent, StoreError> {
	connection
		.query_row("SELECT * FROM chief_inbox_events WHERE id = ?1", [id], event_row)
		.optional()
		.map_err(sqlite_error)?
		.ok_or_else(|| DatabaseError::NotFound.into())
}

fn work_row(row: &Row<'_>) -> rusqlite::Result<ChiefWorkItem> {
	let dispatch_state: String = row.get("dispatch_state")?;
	let dispatch_state = match dispatch_state.as_str() {
		"idle" => ChiefDispatchState::Idle,
		"dispatching" => ChiefDispatchState::Dispatching,
		"running" => ChiefDispatchState::Running,
		"unknown" => ChiefDispatchState::Unknown,
		_ => return Err(rusqlite::Error::InvalidQuery),
	};
	let status: String = row.get("status")?;
	let status = match status.as_str() {
		"open" => ChiefWorkStatus::Open,
		"resolved" => ChiefWorkStatus::Resolved,
		"follow_up" => ChiefWorkStatus::FollowUp,
		"wait" => ChiefWorkStatus::Wait,
		"user_decision" => ChiefWorkStatus::UserDecision,
		_ => return Err(rusqlite::Error::InvalidQuery),
	};
	let kind: String = row.get("kind")?;
	let kind = match kind.as_str() {
		"goal" => ChiefWorkKind::Goal,
		"task" => ChiefWorkKind::Task,
		_ => return Err(rusqlite::Error::InvalidQuery),
	};
	Ok(ChiefWorkItem {
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

fn event_row(row: &Row<'_>) -> rusqlite::Result<ChiefInboxEvent> {
	let disposition: Option<String> = row.get("disposition")?;
	let disposition = match disposition.as_deref() {
		None => None,
		Some("resolved") => Some(ChiefDisposition::Resolved),
		Some("follow_up") => Some(ChiefDisposition::FollowUp),
		Some("wait") => Some(ChiefDisposition::Wait),
		Some("user_decision") => Some(ChiefDisposition::UserDecision),
		_ => return Err(rusqlite::Error::InvalidQuery),
	};
	Ok(ChiefInboxEvent {
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
	mod inbox_carryover;
	mod legacy_setup;
	mod steer_receipts;
	mod task_references;
	mod turn_execution;
	use super::*;
	use tempfile::tempdir;

	fn capacity_failure(work: &str, turn: &str) -> EnqueueChiefEvent {
		EnqueueChiefEvent { source_event_id:format!("failure:{work}:{turn}"),work_item_id:work.into(),event_kind:"chief_turn_completed".into(),
            payload:serde_json::json!({"terminal":{"turn":{"status":"failed","error":{"codexErrorInfo":"serverOverloaded"}}},"threadReadback":{"capacityRetryEligible":true}}).to_string() }
	}

	#[tokio::test]
	async fn capacity_retries_are_bounded_durable_and_claimed_once() {
		let directory = tempdir().unwrap();
		let path = directory.path().join("retry.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store.create_chief_work_item(item("chief", None)).await.unwrap();
		store.bind_chief_thread("chief".into(), "thread".into()).await.unwrap();
		store.begin_chief_dispatch("chief".into()).await.unwrap();
		store.acknowledge_chief_dispatch("chief".into(), "turn-0".into()).await.unwrap();
		for attempt in 1..=3 {
			let turn = format!("turn-{}", attempt - 1);
			let event = store
				.complete_chief_turn_with_event(
					"chief".into(),
					turn.clone(),
					capacity_failure("chief", &turn),
				)
				.await
				.unwrap();
			assert_eq!(event.event_kind, "capacity_retry");
			assert!(store.list_chief_wake_events("chief".into(), 10).await.unwrap().is_empty());
			let retry = store.pending_chief_capacity_retry("chief".into()).await.unwrap().unwrap();
			assert_eq!(retry.attempt, attempt);
			let reopened = SqliteStore::open_test(&path).unwrap();
			assert_eq!(
				reopened.pending_chief_capacity_retry("chief".into()).await.unwrap(),
				Some(retry.clone())
			);
			assert!(
				reopened
					.begin_chief_capacity_retry("chief".into(), event.id, retry.due_at_micros - 1)
					.await
					.is_err()
			);
			reopened
				.begin_chief_capacity_retry("chief".into(), event.id, retry.due_at_micros)
				.await
				.unwrap();
			assert!(
				reopened
					.begin_chief_capacity_retry("chief".into(), event.id, i64::MAX)
					.await
					.is_err()
			);
			assert!(reopened.due_chief_capacity_retries(i64::MAX).await.unwrap().is_empty());
			reopened
				.acknowledge_chief_dispatch("chief".into(), format!("turn-{attempt}"))
				.await
				.unwrap();
		}
		let event = store
			.complete_chief_turn_with_event(
				"chief".into(),
				"turn-3".into(),
				capacity_failure("chief", "turn-3"),
			)
			.await
			.unwrap();
		assert_eq!(event.event_kind, "chief_turn_completed");
		assert!(store.pending_chief_capacity_retry("chief".into()).await.unwrap().is_none());
		assert!(store.due_chief_capacity_retries(i64::MAX).await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn cancellation_new_dispatch_and_unknown_claim_do_not_replay_capacity_retries() {
		let directory = tempdir().unwrap();
		let path = directory.path().join("retry.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		for name in ["cancel", "supersede", "unknown", "resolved"] {
			store.create_chief_work_item(item(name, None)).await.unwrap();
			store.bind_chief_thread(name.into(), format!("thread-{name}")).await.unwrap();
			store.begin_chief_dispatch(name.into()).await.unwrap();
			store.acknowledge_chief_dispatch(name.into(), "failed".into()).await.unwrap();
			let event = store
				.complete_chief_turn_with_event(
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
							.cancel_chief_capacity_retry("unknown".into(), event.id)
							.await
							.is_err()
					);
					store.cancel_chief_capacity_retry(name.into(), event.id).await.unwrap();
				},
				"resolved" => {
					store
						.set_chief_work_status(name.into(), ChiefWorkStatus::Resolved, None)
						.await
						.unwrap();
				},
				"supersede" => {
					store.begin_chief_dispatch(name.into()).await.unwrap();
				},
				_ => {
					store
						.begin_chief_capacity_retry(name.into(), event.id, i64::MAX)
						.await
						.unwrap();
					store.mark_chief_dispatch_unknown(name.into()).await.unwrap();
				},
			}
		}
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert!(store.due_chief_capacity_retries(i64::MAX).await.unwrap().is_empty());
		assert_eq!(
			store.get_chief_work_item("unknown".into()).await.unwrap().dispatch_state,
			ChiefDispatchState::Unknown
		);
	}

	#[tokio::test]
	async fn saved_turn_metrics_require_exact_work_thread_and_turn_after_reopen() {
		let directory = tempdir().unwrap();
		let path = directory.path().join("metrics.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		for work in ["chosen", "peer"] {
			store.create_chief_work_item(item(work, None)).await.unwrap();
		}
		for (work, thread, payload_thread, turn, input) in [
			("chosen", "thread-a", "thread-a", "same/turn", 11),
			("chosen", "thread-b", "thread-b", "same/turn", 22),
			("peer", "thread-a", "thread-a", "peer-turn", 33),
			("chosen", "thread-a", "wrong", "mismatch", 44),
		] {
			store.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id:serde_json::json!(["turn/completed",thread,turn]).to_string(),work_item_id:work.into(),event_kind:"chief_turn_completed".into(),
				payload:serde_json::json!({"terminal":{"threadId":payload_thread,"turn":{"id":turn}},"usage":{"input_tokens":input,"output_tokens":2},"threadReadback":{"tokenUsage":{"marker":"observed"},"assistantMessages":"PRIVATE_TRANSCRIPT"}}).to_string(),
			}).await.unwrap();
		}
		for n in 0..50 {
			store
				.enqueue_chief_event(EnqueueChiefEvent {
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
			.read_chief_turn_metrics(
				"chosen".into(),
				"thread-a".into(),
				vec!["same/turn".into(), "peer-turn".into(), "mismatch".into(), "missing".into()],
			)
			.await
			.unwrap();
		assert_eq!(metrics.len(), 1);
		assert_eq!(metrics[0].turn_id, "same/turn");
		assert_eq!(
			serde_json::from_str::<serde_json::Value>(metrics[0].usage_json.as_ref().unwrap())
				.unwrap()["input_tokens"],
			11
		);
		assert!(!format!("{metrics:?}").contains("PRIVATE_TRANSCRIPT"));
		assert_eq!(
			store
				.read_chief_turn_metrics(
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
				.read_chief_turn_metrics(
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
		let directory = tempdir().unwrap();
		let path = directory.path().join("chief.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store.create_chief_work_item(item("chief", None)).await.unwrap();
		for sequence in 0..3 {
			let input = EnqueueChiefEvent {
				source_event_id: format!("usage-{sequence}"),
				work_item_id: "chief".into(),
				event_kind: "token_usage".into(),
				payload:
					serde_json::json!({"threadId":"thread","turnId":"turn","sequence":sequence})
						.to_string(),
			};
			let first = store.record_chief_observation(input.clone()).await.unwrap();
			assert_eq!(store.record_chief_observation(input).await.unwrap().id, first.id);
			assert_eq!(first.disposition, Some(ChiefDisposition::Resolved));
		}
		assert!(store.list_pending_chief_events(10).await.unwrap().is_empty());
		assert!(store.list_chief_wake_events("chief".into(), 10).await.unwrap().is_empty());
		assert!(store.read_chief_work_events("chief".into(), 10).await.unwrap().is_empty());
		assert_eq!(
			store.get_chief_work_item("chief".into()).await.unwrap().status,
			ChiefWorkStatus::Open
		);
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		store
			.record_chief_observation(EnqueueChiefEvent {
				source_event_id: "other-thread-usage".into(),
				work_item_id: "chief".into(),
				event_kind: "token_usage".into(),
				payload:
					serde_json::json!({"threadId":"other-thread","turnId":"turn","sequence":999})
						.to_string(),
			})
			.await
			.unwrap();
		let event = store
			.read_chief_usage_observation("chief".into(), "thread".into(), "turn".into())
			.await
			.unwrap()
			.unwrap();
		assert_eq!(
			serde_json::from_str::<serde_json::Value>(&event.payload).unwrap()["sequence"],
			2
		);
		assert!(
			store
				.read_chief_usage_observation("chief".into(), "thread".into(), "other".into())
				.await
				.unwrap()
				.is_none()
		);
	}

	fn item(id: &str, parent: Option<&str>) -> ChiefWorkItem {
		ChiefWorkItem {
			id: id.to_owned(),
			parent_goal_id: parent.map(str::to_owned),
			kind: ChiefWorkKind::Goal,
			title: id.to_owned(),
			instructions: "Complete the requested work".to_owned(),
			codex_thread_id: None,
			dispatch_state: ChiefDispatchState::Idle,
			active_turn_id: None,
			status: ChiefWorkStatus::Open,
			next_check_at_micros: None,
			created_at_micros: 1,
			updated_at_micros: 1,
		}
	}

	#[tokio::test]
	async fn misalignment_continuation_requires_exact_review_and_positive_acknowledgment() {
		let directory = tempdir().unwrap();
		let path = directory.path().join("chief.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store.create_chief_work_item(item("chief", None)).await.unwrap();
		store.bind_chief_thread("chief".into(), "thread".into()).await.unwrap();
		store.begin_chief_dispatch("chief".into()).await.unwrap();
		store.acknowledge_chief_dispatch("chief".into(), "failed".into()).await.unwrap();
		let details=serde_json::json!({"detailedExplanation":"Review scope","steer":{"message":"Clarified scope"}}).to_string();
		store
			.record_chief_misalignment("thread".into(), "failed".into(), Some(details))
			.await
			.unwrap();
		store.complete_chief_turn("chief".into(), "failed".into()).await.unwrap();
		let review = store.chief_misalignment("chief".into()).await.unwrap().unwrap();
		let mut stale = review.clone();
		stale.turn_id = "older".into();
		assert!(
			store
				.begin_chief_misalignment_continuation("chief".into(), stale, "stale".into())
				.await
				.is_err()
		);
		let event = store
			.begin_chief_misalignment_continuation("chief".into(), review.clone(), "first".into())
			.await
			.unwrap();
		assert!(store.chief_misalignment("chief".into()).await.unwrap().is_some());
		assert!(
			store
				.begin_chief_misalignment_continuation(
					"chief".into(),
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
				.begin_chief_misalignment_continuation(
					"chief".into(),
					review.clone(),
					"after-restart".into()
				)
				.await
				.is_err()
		);
		store
			.finish_chief_misalignment_continuation("chief".into(), event, review.clone(), None)
			.await
			.unwrap();
		assert!(store.chief_misalignment("chief".into()).await.unwrap().is_some());
		let event = store
			.begin_chief_misalignment_continuation(
				"chief".into(),
				review.clone(),
				"confirmed".into(),
			)
			.await
			.unwrap();
		assert!(
			store
				.finish_chief_misalignment_continuation(
					"chief".into(),
					event,
					review.clone(),
					Some("failed".into())
				)
				.await
				.is_err()
		);
		store
			.finish_chief_misalignment_continuation(
				"chief".into(),
				event,
				review,
				Some("continued".into()),
			)
			.await
			.unwrap();
		assert!(store.chief_misalignment("chief".into()).await.unwrap().is_none());
		let work = store.get_chief_work_item("chief".into()).await.unwrap();
		assert_eq!(work.dispatch_state, ChiefDispatchState::Running);
		assert_eq!(work.active_turn_id.as_deref(), Some("continued"));
	}

	#[tokio::test]
	async fn live_question_provenance_survives_restart_without_promoting_replay() {
		let directory = tempdir().unwrap();
		let path = directory.path().join("chief.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store.create_chief_work_item(item("chief", None)).await.unwrap();
		store.bind_chief_thread("chief".into(), "thread".into()).await.unwrap();
		let record = |id: &str| {
			vec![(
				id.to_owned(),
				serde_json::json!({"id":id,"title":"Question","options":[]}).to_string(),
			)]
		};
		store
			.record_chief_async_questions(
				"thread".into(),
				"turn".into(),
				"history".into(),
				record("old"),
			)
			.await
			.unwrap();
		store
			.record_live_chief_async_questions(
				"thread".into(),
				"turn".into(),
				"history".into(),
				record("old"),
			)
			.await
			.unwrap();
		store
			.record_live_chief_async_questions(
				"thread".into(),
				"turn".into(),
				"live".into(),
				record("new"),
			)
			.await
			.unwrap();
		store
			.record_live_chief_async_questions(
				"thread".into(),
				"turn".into(),
				"live".into(),
				record("new"),
			)
			.await
			.unwrap();
		let questions = store.read_chief_async_questions("chief".into()).await.unwrap();
		assert_eq!(questions.len(), 2);
		assert!(!questions[0].arrived_live);
		assert!(questions[1].arrived_live);
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		store.refresh_chief_async_projection("thread".into()).await.unwrap();
		assert!(store.read_chief_async_questions("chief".into()).await.unwrap().is_empty());
		assert!(
			store
				.replace_chief_async_projection(
					"chief".into(),
					"thread".into(),
					None,
					questions,
					vec![]
				)
				.await
				.unwrap()
		);
		let mut questions = store.read_chief_async_questions("chief".into()).await.unwrap();
		assert!(!questions[0].arrived_live);
		assert!(questions[1].arrived_live);
		// Input provenance is not authority: a changed native question is history.
		questions[1].question_json =
			serde_json::json!({"id":"new","title":"Changed","options":[]}).to_string();
		store.refresh_chief_async_projection("thread".into()).await.unwrap();
		store
			.replace_chief_async_projection(
				"chief".into(),
				"thread".into(),
				None,
				questions,
				vec![],
			)
			.await
			.unwrap();
		assert!(
			store
				.read_chief_async_questions("chief".into())
				.await
				.unwrap()
				.iter()
				.all(|q| !q.arrived_live)
		);
		assert!(
			store
				.skip_chief_async_question("chief".into(), "thread".into(), "new".into())
				.await
				.unwrap()
		);
		store.resolve_chief_async_questions("thread".into(), vec!["old".into()]).await.unwrap();
		assert!(store.read_chief_async_questions("chief".into()).await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn new_prompt_retires_questions_without_replay_or_answer_side_effects() {
		let directory = tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("chief.sqlite3")).unwrap();
		store.create_chief_work_item(item("chief", None)).await.unwrap();
		store.bind_chief_thread("chief".into(), "thread".into()).await.unwrap();
		let record = |id: &str| {
			vec![(
				id.to_owned(),
				serde_json::json!({"id":id,"title":"Question","options":[]}).to_string(),
			)]
		};
		store
			.record_chief_async_questions(
				"thread".into(),
				"turn".into(),
				"item".into(),
				record("q1"),
			)
			.await
			.unwrap();
		let answer = EnqueueChiefEvent {
			source_event_id: "reply".into(),
			work_item_id: "chief".into(),
			event_kind: "user_message".into(),
			payload: r#"{"text":"Answer","asyncQuestionReply":true}"#.into(),
		};
		store.enqueue_chief_event(answer).await.unwrap();
		assert_eq!(store.read_chief_async_questions("chief".into()).await.unwrap().len(), 1);
		let prompt = EnqueueChiefEvent {
			source_event_id: "prompt".into(),
			work_item_id: "chief".into(),
			event_kind: "user_message".into(),
			payload: r#"{"text":"New work"}"#.into(),
		};
		store.enqueue_chief_event(prompt.clone()).await.unwrap();
		store
			.record_chief_async_questions(
				"thread".into(),
				"turn".into(),
				"item".into(),
				record("q1"),
			)
			.await
			.unwrap();
		assert!(store.read_chief_async_questions("chief".into()).await.unwrap().is_empty());
		store
			.record_chief_async_questions(
				"thread".into(),
				"turn".into(),
				"item2".into(),
				record("q2"),
			)
			.await
			.unwrap();
		store.enqueue_chief_event(prompt).await.unwrap();
		assert_eq!(store.read_chief_async_questions("chief".into()).await.unwrap().len(), 1);
		// Queued prompt delivery retires questions that arrived while waiting.
		let event = store
			.read_chief_work_events("chief".into(), 10)
			.await
			.unwrap()
			.into_iter()
			.find(|event| event.source_event_id == "prompt")
			.unwrap();
		store.begin_chief_dispatch_with_events("chief".into(), vec![event.id]).await.unwrap();
		assert!(store.read_chief_async_questions("chief".into()).await.unwrap().is_empty());
		store.acknowledge_chief_dispatch("chief".into(), "active".into()).await.unwrap();
		store
			.record_chief_async_questions(
				"thread".into(),
				"active".into(),
				"item3".into(),
				record("q3"),
			)
			.await
			.unwrap();
		let rejected = store
			.begin_chief_steer(
				"chief".into(),
				"active".into(),
				"rejected".into(),
				r#"{"text":"New work"}"#.into(),
			)
			.await
			.unwrap();
		store.finish_chief_steer(rejected, false).await.unwrap();
		assert_eq!(store.read_chief_async_questions("chief".into()).await.unwrap().len(), 1);
		let reply = store
			.begin_chief_steer(
				"chief".into(),
				"active".into(),
				"reply".into(),
				r#"{"text":"Answer","asyncQuestionReply":true}"#.into(),
			)
			.await
			.unwrap();
		store.finish_chief_steer(reply, true).await.unwrap();
		assert_eq!(store.read_chief_async_questions("chief".into()).await.unwrap().len(), 1);
		let accepted = store
			.begin_chief_steer(
				"chief".into(),
				"active".into(),
				"accepted".into(),
				r#"{"text":"New work"}"#.into(),
			)
			.await
			.unwrap();
		store.finish_chief_steer(accepted, true).await.unwrap();
		assert!(store.read_chief_async_questions("chief".into()).await.unwrap().is_empty());
	}
	#[tokio::test]
	async fn async_answers_only_dispatch_once_to_the_exact_owner_and_never_wake() {
		let directory = tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("chief.sqlite3")).unwrap();
		store.create_chief_work_item(item("chief", None)).await.unwrap();
		store.create_chief_work_item(item("worker", Some("chief"))).await.unwrap();
		store.bind_chief_thread("chief".into(), "chief-thread".into()).await.unwrap();
		store.bind_chief_thread("worker".into(), "worker-thread".into()).await.unwrap();
		let event = store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: "answer-1".into(),
				work_item_id: "worker".into(),
				event_kind: "async_question_answer".into(),
				payload: r#"{"text":"Europe","source":"user"}"#.into(),
			})
			.await
			.unwrap();
		assert!(store.list_undelivered_chief_events(10).await.unwrap().is_empty());
		assert!(store.list_chief_wake_events("chief".into(), 10).await.unwrap().is_empty());
		assert!(
			store.begin_chief_dispatch_with_events("chief".into(), vec![event.id]).await.is_err()
		);
		assert_eq!(
			store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
			ChiefDispatchState::Idle
		);
		store.begin_chief_dispatch_with_events("worker".into(), vec![event.id]).await.unwrap();
		store.acknowledge_chief_dispatch("worker".into(), "turn-1".into()).await.unwrap();
		let receipt = store.get_chief_inbox_event(event.id).await.unwrap();
		assert_eq!(receipt.delivered_turn_id.as_deref(), Some("turn-1"));
		assert_eq!(receipt.work_item_id, "worker");
		store.complete_chief_turn("worker".into(), "turn-1".into()).await.unwrap();
		assert!(
			store.begin_chief_dispatch_with_events("worker".into(), vec![event.id]).await.is_err()
		);
		assert_eq!(
			store.get_chief_work_item("worker".into()).await.unwrap().dispatch_state,
			ChiefDispatchState::Idle
		);
		assert!(store.list_chief_wake_events("chief".into(), 10).await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn delivery_attention_recovers_without_consuming_saved_user_input() {
		let directory = tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("chief.sqlite3")).unwrap();
		store.create_chief_work_item(item("chief", None)).await.unwrap();
		store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: "user-1".into(),
				work_item_id: "chief".into(),
				event_kind: "user_message".into(),
				payload: r#"{"text":"continue"}"#.into(),
			})
			.await
			.unwrap();
		store.record_chief_delivery_failure("chief".into(), "Unavailable".into()).await.unwrap();
		store.record_chief_thread_in_use("chief".into(), "Open elsewhere".into()).await.unwrap();
		store.record_chief_thread_in_use("chief".into(), "Open elsewhere".into()).await.unwrap();
		assert!(
			store
				.list_pending_chief_events(10)
				.await
				.unwrap()
				.iter()
				.any(|e| e.event_kind == "thread_in_use_needs_attention")
		);
		assert_eq!(store.list_pending_chief_events(10).await.unwrap().len(), 2);
		store.resolve_chief_delivery_failure("chief".into()).await.unwrap();
		let pending = store.list_pending_chief_events(10).await.unwrap();
		assert_eq!(pending.len(), 1);
		assert_eq!(pending[0].event_kind, "user_message");
		assert!(pending[0].delivered_turn_id.is_none());
		store.record_chief_delivery_failure("chief".into(), "Later failure".into()).await.unwrap();
		assert_eq!(store.read_chief_work_events("chief".into(), 10).await.unwrap().len(), 4);
	}

	#[tokio::test]
	async fn redispatch_invalidates_manager_acceptance_and_records_instruction_atomically() {
		let directory = tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("chief.sqlite3")).unwrap();
		store.create_chief_work_item(item("chief", None)).await.unwrap();
		store.create_chief_manager(item("manager", Some("chief")), None).await.unwrap();
		store.bind_chief_thread("manager".into(), "manager-thread".into()).await.unwrap();
		store
			.set_chief_work_status("manager".into(), ChiefWorkStatus::Resolved, None)
			.await
			.unwrap();
		store
			.begin_chief_dispatch_with_input(
				"manager".into(),
				vec![],
				Some("Check the revised result".into()),
			)
			.await
			.unwrap();
		assert_eq!(
			store.get_chief_work_item("manager".into()).await.unwrap().status,
			ChiefWorkStatus::Open
		);
		assert!(
			store
				.begin_chief_dispatch_with_input(
					"manager".into(),
					vec![],
					Some("Do not duplicate".into())
				)
				.await
				.is_err()
		);
		store.acknowledge_chief_dispatch("manager".into(), "new-turn".into()).await.unwrap();
		let history = store.read_chief_work_events("manager".into(), 10).await.unwrap();
		assert_eq!(history.len(), 1);
		assert_eq!(history[0].event_kind, "work_instruction");
		assert_eq!(history[0].delivered_turn_id.as_deref(), Some("new-turn"));
		assert_eq!(
			serde_json::from_str::<serde_json::Value>(&history[0].payload).unwrap()["text"],
			"Check the revised result"
		);
		assert!(store.list_undelivered_chief_events(10).await.unwrap().is_empty());
		store.complete_chief_turn("manager".into(), "new-turn".into()).await.unwrap();
		assert_eq!(
			store.get_chief_work_item("manager".into()).await.unwrap().status,
			ChiefWorkStatus::Open
		);
	}

	#[tokio::test]
	async fn connection_attention_closes_and_rearms_without_resolving_work() {
		let directory = tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("chief.sqlite3")).unwrap();
		store.create_chief_work_item(item("chief", None)).await.unwrap();
		store
			.record_chief_connection_failure("chief".into(), "Refresh quota".into())
			.await
			.unwrap();
		store
			.record_chief_connection_failure("chief".into(), "Retry pending".into())
			.await
			.unwrap();
		assert_eq!(store.read_chief_work_events("chief".into(), 10).await.unwrap().len(), 2);
		store.resolve_chief_connection_failure("chief".into()).await.unwrap();
		let saved = store.read_chief_work_events("chief".into(), 10).await.unwrap();
		assert_eq!(saved[0].disposition, Some(ChiefDisposition::Resolved));
		assert_eq!(
			store.get_chief_work_item("chief".into()).await.unwrap().status,
			ChiefWorkStatus::Open
		);
		store
			.record_chief_connection_failure("chief".into(), "Later failure".into())
			.await
			.unwrap();
		let saved = store.read_chief_work_events("chief".into(), 10).await.unwrap();
		assert_eq!(saved.len(), 3);
		assert_eq!(saved.iter().filter(|event| event.disposition.is_none()).count(), 1);
	}

	#[tokio::test]
	async fn chief_request_receipts_and_handled_inputs_leave_work_judgment_and_newer_events_intact()
	{
		let directory = tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("chief.sqlite3")).unwrap();
		store.create_chief_work_item(item("chief", None)).await.unwrap();
		store.bind_chief_thread("chief".into(), "thread".into()).await.unwrap();
		store
			.set_chief_work_status("chief".into(), ChiefWorkStatus::Wait, Some(100))
			.await
			.unwrap();
		let initial = store.get_chief_work_item("chief".into()).await.unwrap();
		for kind in ["permission_pending", "user_input_pending", "server_request_pending"] {
			let request = store
				.enqueue_chief_event(EnqueueChiefEvent {
					source_event_id: kind.into(),
					work_item_id: "chief".into(),
					event_kind: kind.into(),
					payload: "{}".into(),
				})
				.await
				.unwrap();
			let acknowledged = store.acknowledge_chief_request_event(request.id).await.unwrap();
			assert_eq!(acknowledged.disposition, Some(ChiefDisposition::Resolved));
			assert!(store.acknowledge_chief_request_event(request.id).await.is_err());
		}
		assert_eq!(store.get_chief_work_item("chief".into()).await.unwrap(), initial);
		let mut retained = Vec::new();
		for (index, status) in [(1, "failed"), (2, "interrupted"), (3, "completed")] {
			let input = store
				.enqueue_chief_event(EnqueueChiefEvent {
					source_event_id: format!("user:{index}"),
					work_item_id: "chief".into(),
					event_kind: "user_message".into(),
					payload: "user input".into(),
				})
				.await
				.unwrap();
			assert!(store.acknowledge_chief_request_event(input.id).await.is_err());
			store.begin_chief_dispatch_with_events("chief".into(), vec![input.id]).await.unwrap();
			let turn = format!("turn:{index}");
			store.acknowledge_chief_dispatch("chief".into(), turn.clone()).await.unwrap();
			if status == "completed" {
				let newer = store
					.enqueue_chief_event(EnqueueChiefEvent {
						source_event_id: "newer-user".into(),
						work_item_id: "chief".into(),
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
				.complete_chief_turn_with_event(
					"chief".into(),
					turn,
					EnqueueChiefEvent {
						source_event_id: format!("terminal:{index}"),
						work_item_id: "chief".into(),
						event_kind: "chief_turn_completed".into(),
						payload: serde_json::json!({"terminal":{"turn":{"status":status}}})
							.to_string(),
					},
				)
				.await
				.unwrap();
		}
		let pending = store.list_pending_chief_events(100).await.unwrap();
		assert_eq!(pending.iter().map(|event| event.id).collect::<Vec<_>>(), retained);
		let work = store.get_chief_work_item("chief".into()).await.unwrap();
		assert_eq!(work.status, ChiefWorkStatus::Wait);
		assert_eq!(work.next_check_at_micros, Some(100));
	}

	#[tokio::test]
	async fn chief_due_notifications_filter_before_limit_and_user_messages_can_wake() {
		let directory = tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("chief.sqlite3")).unwrap();
		for id in ["first", "second"] {
			store.create_chief_work_item(item(id, None)).await.unwrap();
			store.set_chief_work_status(id.into(), ChiefWorkStatus::Wait, Some(100)).await.unwrap();
		}
		store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: serde_json::json!(["followup_due", "first", 100]).to_string(),
				work_item_id: "first".into(),
				event_kind: "followup_due".into(),
				payload: "{}".into(),
			})
			.await
			.unwrap();
		assert_eq!(
			store.list_unnotified_due_chief_work_items(100, 1).await.unwrap()[0].id,
			"second"
		);
		store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: "user-command:1".into(),
				work_item_id: "first".into(),
				event_kind: "user_message".into(),
				payload: "Hello Chief".into(),
			})
			.await
			.unwrap();
		assert!(
			store
				.list_undelivered_chief_events(10)
				.await
				.unwrap()
				.iter()
				.any(|event| event.event_kind == "user_message")
		);
	}

	#[tokio::test]
	async fn chief_reads_filter_before_limits_and_terminal_receipts_do_not_change_judgment() {
		let directory = tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("chief.sqlite3")).unwrap();
		store.create_chief_work_item(item("chief", None)).await.unwrap();
		store.begin_chief_thread_creation("chief".into()).await.unwrap();
		assert!(store.begin_chief_thread_creation("chief".into()).await.is_err());
		store
			.acknowledge_chief_thread_creation("chief".into(), "opaque-thread".into())
			.await
			.unwrap();
		for (source, kind) in
			[("unrelated-1", "other"), ("unrelated-2", "other"), ("wake", "automation_result")]
		{
			store
				.enqueue_chief_event(EnqueueChiefEvent {
					source_event_id: source.into(),
					work_item_id: "chief".into(),
					event_kind: kind.into(),
					payload: String::new(),
				})
				.await
				.unwrap();
		}
		let event = store.list_undelivered_chief_events(1).await.unwrap().remove(0);
		assert_eq!(event.source_event_id, "wake");
		store
			.set_chief_work_status("chief".into(), ChiefWorkStatus::Wait, Some(100))
			.await
			.unwrap();
		store.begin_chief_dispatch_with_events("chief".into(), vec![event.id]).await.unwrap();
		store.acknowledge_chief_dispatch("chief".into(), "turn-1".into()).await.unwrap();
		assert!(store.list_undelivered_chief_events(1).await.unwrap().is_empty());
		assert_eq!(
			store.list_chief_events_for_turn("turn-1".into(), 1).await.unwrap()[0].id,
			event.id
		);
		let receipt = store
			.complete_chief_turn_with_event(
				"chief".into(),
				"turn-1".into(),
				EnqueueChiefEvent {
					source_event_id: "chief-terminal".into(),
					work_item_id: "chief".into(),
					event_kind: "chief_turn_completed".into(),
					payload: "chief reply".into(),
				},
			)
			.await
			.unwrap();
		assert_eq!(receipt.disposition, Some(ChiefDisposition::Resolved));
		let work = store.get_chief_work_item("chief".into()).await.unwrap();
		assert_eq!(work.status, ChiefWorkStatus::Wait);
		assert_eq!(work.next_check_at_micros, Some(100));
		assert_eq!(store.list_pending_chief_events(100).await.unwrap().len(), 3);
		assert!(matches!(
			store.read_chief_snapshot(1, 1, 1).await.unwrap(),
			ChiefStoreSnapshot::CapacityExceeded { pending_events: 3, .. }
		));
	}

	#[tokio::test]
	async fn chief_thread_creation_unknown_survives_reopen_and_new_task_turn_revokes_old_acceptance()
	 {
		let directory = tempdir().unwrap();
		let path = directory.path().join("chief.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store.create_chief_work_item(item("unknown", None)).await.unwrap();
		store.begin_chief_thread_creation("unknown".into()).await.unwrap();
		store.mark_chief_dispatch_unknown("unknown".into()).await.unwrap();
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert!(store.begin_chief_thread_creation("unknown".into()).await.is_err());
		let mut task = item("task", None);
		task.kind = ChiefWorkKind::Task;
		store.create_chief_work_item(task).await.unwrap();
		store.begin_chief_thread_creation("task".into()).await.unwrap();
		store.acknowledge_chief_thread_creation("task".into(), "task-thread".into()).await.unwrap();
		store.set_chief_work_status("task".into(), ChiefWorkStatus::Resolved, None).await.unwrap();
		let dispatched = store.begin_chief_dispatch("task".into()).await.unwrap();
		assert_eq!(dispatched.status, ChiefWorkStatus::Open);
	}

	#[tokio::test]
	async fn chief_transport_close_preserves_running_turn_as_unknown_across_reopen() {
		let directory = tempdir().unwrap();
		let path = directory.path().join("chief.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store.create_chief_work_item(item("worker", None)).await.unwrap();
		store.bind_chief_thread("worker".into(), "opaque-thread".into()).await.unwrap();
		store.begin_chief_dispatch("worker".into()).await.unwrap();
		store
			.acknowledge_chief_dispatch("worker".into(), "exact-running-turn".into())
			.await
			.unwrap();
		let unknown = store.mark_chief_dispatch_unknown("worker".into()).await.unwrap();
		assert_eq!(unknown.dispatch_state, ChiefDispatchState::Unknown);
		assert_eq!(unknown.active_turn_id.as_deref(), Some("exact-running-turn"));
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert_eq!(store.get_chief_work_item("worker".into()).await.unwrap(), unknown);
		assert!(store.begin_chief_dispatch("worker".into()).await.is_err());
		assert!(
			store.reconcile_chief_dispatch("worker".into(), "different-turn".into()).await.is_err()
		);
		let reconciled = store
			.reconcile_chief_dispatch("worker".into(), "exact-running-turn".into())
			.await
			.unwrap();
		assert_eq!(reconciled.dispatch_state, ChiefDispatchState::Running);
		assert!(store.begin_chief_dispatch("worker".into()).await.is_err());
		store.revalidate().await.unwrap();
	}

	#[tokio::test]
	async fn chief_dispatch_delivery_and_unknown_state_survive_restart_without_replay() {
		let directory = tempdir().unwrap();
		let path = directory.path().join("chief.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store.create_chief_work_item(item("chief", None)).await.unwrap();
		store.bind_chief_thread("chief".into(), "opaque-thread".into()).await.unwrap();
		let event = store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: "wake:1".into(),
				work_item_id: "chief".into(),
				event_kind: "automation_result".into(),
				payload: String::new(),
			})
			.await
			.unwrap();
		assert!(
			store
				.begin_chief_dispatch_with_events("chief".into(), vec![event.id, -1])
				.await
				.is_err()
		);
		assert_eq!(
			store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
			ChiefDispatchState::Idle
		);
		assert_eq!(store.list_pending_chief_events(10).await.unwrap()[0].delivered_turn_id, None);
		store.begin_chief_dispatch_with_events("chief".into(), vec![event.id]).await.unwrap();
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert_eq!(
			store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
			ChiefDispatchState::Dispatching
		);
		assert!(store.begin_chief_dispatch("chief".into()).await.is_err());
		assert_eq!(
			store.list_pending_chief_events(10).await.unwrap()[0].delivered_turn_id,
			Some(String::new())
		);
		store.mark_chief_dispatch_unknown("chief".into()).await.unwrap();
		assert!(store.begin_chief_dispatch("chief".into()).await.is_err());
		store.reconcile_chief_dispatch("chief".into(), "opaque-turn".into()).await.unwrap();
		assert!(store.complete_chief_turn("chief".into(), "wrong-turn".into()).await.is_err());
		let terminal = EnqueueChiefEvent {
			source_event_id: "completion:1".into(),
			work_item_id: "chief".into(),
			event_kind: "turn_completed".into(),
			payload: "positive exact-turn evidence".into(),
		};
		store
			.complete_chief_turn_with_event("chief".into(), "opaque-turn".into(), terminal)
			.await
			.unwrap();
		assert_eq!(
			store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
			ChiefDispatchState::Idle
		);
		let pending = store.list_pending_chief_events(10).await.unwrap();
		assert_eq!(pending.len(), 2);
		assert_eq!(pending[0].delivered_turn_id.as_deref(), Some("opaque-turn"));
		assert!(pending.iter().all(|event| event.disposition.is_none()));
		store.revalidate().await.unwrap();
	}

	#[tokio::test]
	async fn chief_graph_rejects_cycles_and_preserves_opaque_binding() {
		let directory = tempdir().unwrap();
		let path = directory.path().join("chief.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store.create_chief_work_item(item("goal", None)).await.unwrap();
		store.create_chief_work_item(item("a", Some("goal"))).await.unwrap();
		store.create_chief_work_item(item("b", Some("goal"))).await.unwrap();
		store.create_chief_work_item(item("c", Some("goal"))).await.unwrap();
		store.add_chief_dependency("a".into(), "b".into()).await.unwrap();
		store.add_chief_dependency("b".into(), "c".into()).await.unwrap();
		assert!(store.add_chief_dependency("c".into(), "a".into()).await.is_err());
		assert!(store.add_chief_dependency("a".into(), "a".into()).await.is_err());
		store
			.with_connection(|connection| {
				assert!(
					connection
						.execute("INSERT INTO chief_dependencies VALUES ('c', 'a')", [])
						.is_err()
				);
				assert!(
					connection
						.execute(
							"UPDATE chief_work_items SET parent_goal_id = 'c' WHERE id = 'goal'",
							[]
						)
						.is_err()
				);
				Ok(())
			})
			.unwrap();
		assert!(store.create_chief_work_item(item("missing", Some("unknown"))).await.is_err());
		let opaque = "thread:provider/opaque not-a-uuid".to_owned();
		store.bind_chief_thread("a".into(), opaque.clone()).await.unwrap();
		assert!(store.bind_chief_thread("a".into(), "other".into()).await.is_err());
		assert!(store.bind_chief_thread("b".into(), opaque.clone()).await.is_err());
		drop(store);
		let reopened = SqliteStore::open_test(&path).unwrap();
		assert_eq!(reopened.list_chief_dependencies().await.unwrap().len(), 2);
		assert_eq!(
			reopened
				.list_chief_work_items()
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
	async fn chief_inbox_survives_restart_deduplicates_and_disposes_exactly_once() {
		let directory = tempdir().unwrap();
		let path = directory.path().join("chief.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store.create_chief_work_item(item("goal", None)).await.unwrap();
		let input = EnqueueChiefEvent {
			source_event_id: "source:event:1".into(),
			work_item_id: "goal".into(),
			event_kind: "thread_completed".into(),
			payload: "result".into(),
		};
		let first = store.enqueue_chief_event(input.clone()).await.unwrap();
		assert_eq!(first, store.enqueue_chief_event(input.clone()).await.unwrap());
		let mut changed = input.clone();
		changed.payload = "different".into();
		assert!(matches!(
			store.enqueue_chief_event(changed).await,
			Err(StoreError::IdempotencyConflict)
		));
		let mut second_input = input.clone();
		second_input.source_event_id = "source:event:2".into();
		let second = store.enqueue_chief_event(second_input).await.unwrap();
		assert_eq!(store.list_pending_chief_events(1).await.unwrap(), vec![first.clone()]);
		drop(store);
		let reopened = SqliteStore::open_test(&path).unwrap();
		assert_eq!(reopened.list_pending_chief_events(100).await.unwrap().len(), 2);
		let (one, two) = tokio::join!(
			reopened.dispose_chief_event(
				first.id,
				ChiefDisposition::Wait,
				"check again".into(),
				Some(100)
			),
			reopened.dispose_chief_event(
				first.id,
				ChiefDisposition::Wait,
				"duplicate processor".into(),
				Some(100)
			)
		);
		assert_ne!(one.is_ok(), two.is_ok());
		assert_eq!(reopened.list_pending_chief_events(100).await.unwrap(), vec![second]);
		assert_eq!(reopened.list_due_chief_work_items(100, 100).await.unwrap().len(), 1);
		assert!(reopened.enqueue_chief_event(input).await.unwrap().disposition.is_some());
		reopened.revalidate().await.unwrap();
	}
}
