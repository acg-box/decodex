//! Read existing inbox facts without creating a collaboration ledger or wake.
use super::{AgentInboxEvent, SqliteStore, StoreError, error, read_event};

pub struct AgentCollaborationRecord {
	pub event: AgentInboxEvent,
	pub instruction_event_id: Option<i64>,
	pub recipient_work_id: Option<String>,
}

pub struct AgentCollaborationQuestion {
	pub work: String,
	pub question: crate::AgentAsyncQuestion,
	pub pending: bool,
}

impl SqliteStore {
	pub async fn read_agent_collaboration_questions(
		&self,
		visible: Vec<String>,
	) -> Result<Vec<AgentCollaborationQuestion>, StoreError> {
		let visible = serde_json::to_string(&visible)
			.map_err(|_| StoreError::InvalidInput("invalid collaboration owners"))?;
		self.run(move |connection| {
			connection.prepare("SELECT q.work_id,q.thread_id,q.turn_id,q.item_id,q.question_id,q.question_json,q.arrived_live, NOT EXISTS(SELECT 1 FROM agent_async_answers a WHERE a.work_id=q.work_id AND a.thread_id=q.thread_id AND a.question_id IN(q.question_id,q.item_id)) AND NOT EXISTS(SELECT 1 FROM agent_async_skips s WHERE s.work_id=q.work_id AND s.thread_id=q.thread_id AND s.question_id=q.question_id) AS pending FROM agent_async_questions q JOIN agent_work_items w ON w.id=q.work_id AND w.codex_thread_id=q.thread_id WHERE q.work_id IN (SELECT value FROM json_each(?1)) AND NOT EXISTS(SELECT 1 FROM agent_async_recovery r WHERE r.work_id=q.work_id AND r.thread_id=q.thread_id) ORDER BY pending DESC,q.created_at_micros DESC,q.rowid DESC LIMIT 101").map_err(error::sqlite_error)?
			.query_map([visible], |row| Ok(AgentCollaborationQuestion { work:row.get(0)?,question:crate::AgentAsyncQuestion {thread_id:row.get(1)?,turn_id:row.get(2)?,item_id:row.get(3)?,question_id:row.get(4)?,question_json:row.get(5)?,arrived_live:row.get(6)?},pending:row.get(7)?})).map_err(error::sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(|error|error::sqlite_error(error).into())
		}).await
	}

	pub async fn read_agent_collaboration(
		&self,
		visible: Vec<String>,
		before: Option<i64>,
	) -> Result<Vec<AgentCollaborationRecord>, StoreError> {
		let visible = serde_json::to_string(&visible)
			.map_err(|_| StoreError::InvalidInput("invalid collaboration owners"))?;
		self.run(move |connection| {
			let tx = connection.transaction().map_err(error::sqlite_error)?;
			let ids = tx.prepare(
				"SELECT e.id, (SELECT i.id FROM agent_inbox_events i WHERE e.event_kind='worker_turn_completed' AND i.work_item_id=e.work_item_id AND i.event_kind='work_instruction' AND i.id<e.id AND i.delivered_turn_id<>'' AND i.delivered_turn_id=json_extract(e.payload,'$.terminal.turn.id') AND json_extract(i.payload,'$.threadId')=json_extract(e.payload,'$.terminal.threadId') ORDER BY i.id DESC LIMIT 1),e.delivery_work_item_id FROM agent_inbox_events e WHERE e.work_item_id IN (SELECT value FROM json_each(?1)) AND e.event_kind IN ('work_instruction','worker_turn_completed','collaboration_refused') AND (?2 IS NULL OR e.id<?2) ORDER BY e.id DESC LIMIT 101"
			).map_err(error::sqlite_error)?
			.query_map(rusqlite::params![visible, before], |row| Ok((row.get::<_, i64>(0)?,row.get::<_, Option<i64>>(1)?,row.get::<_,Option<String>>(2)?)))
			.map_err(error::sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(error::sqlite_error)?;
			let records = ids.into_iter().map(|(id,instruction_event_id,recipient_work_id)| {
				Ok(AgentCollaborationRecord {event:read_event(&tx,id)?,instruction_event_id,recipient_work_id})
			}).collect::<Result<Vec<_>,StoreError>>()?;
			tx.commit().map_err(error::sqlite_error)?;
			Ok(records)
		}).await
	}
}
