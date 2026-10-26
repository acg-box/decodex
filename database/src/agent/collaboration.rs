//! Read existing inbox facts without creating a collaboration ledger or wake.
use rusqlite::{Connection, OptionalExtension as _};

use crate::agent::{self, AgentInboxEvent, SqliteStore, StoreError, error};

pub struct AgentCollaborationRecord {
	pub event: AgentInboxEvent,
	pub instruction_event_id: Option<i64>,
	pub recipient_work_id: Option<String>,
	pub peer: Option<PeerExchange>,
}

pub struct PeerExchange {
	pub work_id: String,
	pub request_id: String,
	pub instruction_id: String,
	pub requesting_agent: String,
	pub recipient_agent: String,
	pub current: bool,
	pub delivery_thread: Option<String>,
	pub state: String,
	pub outcome: Option<String>,
	pub note: Option<String>,
}

pub struct AgentCollaborationQuestion {
	pub work: String,
	pub question: crate::AgentAsyncQuestion,
	pub pending: bool,
	pub answer: Option<AgentInboxEvent>,
	pub answer_outcome: Option<String>,
}

impl SqliteStore {
	pub async fn read_agent_collaboration_questions(
		&self,
		visible: Vec<String>,
	) -> Result<Vec<AgentCollaborationQuestion>, StoreError> {
		let visible = serde_json::to_string(&visible)
			.map_err(|_| StoreError::InvalidInput("invalid collaboration owners"))?;

		self.run(move |connection| {
			let tx = connection.transaction().map_err(error::sqlite_error)?;
			let mut rows = tx.prepare("SELECT q.work_id,q.thread_id,q.turn_id,q.item_id,q.question_id,q.question_json,q.arrived_live, NOT EXISTS(SELECT 1 FROM agent_async_answers a WHERE a.work_id=q.work_id AND a.thread_id=q.thread_id AND a.question_id IN(q.question_id,q.item_id)) AND NOT EXISTS(SELECT 1 FROM agent_async_skips s WHERE s.work_id=q.work_id AND s.thread_id=q.thread_id AND s.question_id=q.question_id) AS pending FROM agent_async_questions q JOIN agent_work_items w ON w.id=q.work_id AND w.codex_thread_id=q.thread_id WHERE q.work_id IN (SELECT value FROM json_each(?1)) AND NOT EXISTS(SELECT 1 FROM agent_async_recovery r WHERE r.work_id=q.work_id AND r.thread_id=q.thread_id) ORDER BY pending DESC,q.created_at_micros DESC,q.rowid DESC LIMIT 101").map_err(error::sqlite_error)?
			.query_map([visible], |row| Ok(AgentCollaborationQuestion { work:row.get(0)?,question:crate::AgentAsyncQuestion {thread_id:row.get(1)?,turn_id:row.get(2)?,item_id:row.get(3)?,question_id:row.get(4)?,question_json:row.get(5)?,arrived_live:row.get(6)?},pending:row.get(7)?,answer:None,answer_outcome:None})).map_err(error::sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(error::sqlite_error)?;

			for row in &mut rows { reply(&tx,row)?; }

			tx.commit().map_err(error::sqlite_error)?;

			Ok(rows)
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
				"SELECT e.id, (SELECT i.id FROM agent_inbox_events i WHERE e.event_kind='worker_turn_completed' AND i.work_item_id=e.work_item_id AND i.event_kind='work_instruction' AND i.id<e.id AND i.delivered_turn_id<>'' AND i.delivered_turn_id=json_extract(e.payload,'$.terminal.turn.id') AND json_extract(i.payload,'$.threadId')=json_extract(e.payload,'$.terminal.threadId') ORDER BY i.id DESC LIMIT 1),e.delivery_work_item_id FROM agent_inbox_events e WHERE e.work_item_id IN (SELECT value FROM json_each(?1)) AND e.event_kind IN ('work_instruction','worker_turn_completed','collaboration_refused','collaboration_request','collaboration_result') AND (?2 IS NULL OR e.id<?2) ORDER BY e.id DESC LIMIT 101"
			).map_err(error::sqlite_error)?
			.query_map(rusqlite::params![visible, before], |row| Ok((row.get::<_, i64>(0)?,row.get::<_, Option<i64>>(1)?,row.get::<_,Option<String>>(2)?)))
			.map_err(error::sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(error::sqlite_error)?;
			let records = ids.into_iter().map(|(id,instruction_event_id,recipient_work_id)| {
				let event = agent::read_event(&tx,id)?;
				let context = peer(&tx,&event)?;

				Ok(AgentCollaborationRecord {event,instruction_event_id:context.as_ref().map(|(_,id)|*id).or(instruction_event_id),recipient_work_id,peer:context.map(|(peer,_)|peer)})
			}).collect::<Result<Vec<_>,StoreError>>()?;

			tx.commit().map_err(error::sqlite_error)?;

			Ok(records)
		}).await
	}
}

/// Append exact transport evidence without changing the immutable request body or waking work.
pub(super) fn record_delivery(
	c: &Connection,
	agent: &str,
	thread: Option<&str>,
	turn: &str,
) -> Result<(), StoreError> {
	let Some(thread) = thread else { return Ok(()) };
	let now = crate::unix_micros()?;

	c.execute("INSERT OR IGNORE INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) SELECT json_array('collaboration_delivery',id),?1,'collaboration_delivery',json_object('eventId',id,'threadId',?2,'turnId',?3),?4,'resolved','Native admission observed; contribution acceptance is separate.',?4 FROM agent_inbox_events WHERE event_kind='collaboration_request' AND delivery_work_item_id=?1 AND delivered_turn_id='' AND disposition IS NULL",rusqlite::params![agent,thread,turn,now]).map_err(error::sqlite_error)?;

	Ok(())
}

fn reply(c: &Connection, row: &mut AgentCollaborationQuestion) -> Result<(), StoreError> {
	let event: Option<i64> = c.query_row(
		"SELECT id FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind IN ('async_question_answer','user_message') AND json_extract(payload,'$.threadId')=?2 AND json_extract(payload,'$.asyncQuestionId')=?3 ORDER BY id DESC LIMIT 1",
		rusqlite::params![row.work,row.question.thread_id,row.question.question_id], |r|r.get(0)
	).optional().map_err(error::sqlite_error)?;

	if let Some(id) = event {
		let answer = agent::read_event(c, id)?;

		if let Some(turn) = answer.delivered_turn_id.as_ref().filter(|id| !id.is_empty()) {
			row.answer_outcome = c.query_row(
				"SELECT json_extract(payload,'$.terminal.turn.status') FROM agent_inbox_events WHERE work_item_id=?1 AND source_event_id=json_array('turn/completed',?2,?3) AND event_kind IN ('agent_turn_completed','worker_turn_completed') AND json_extract(payload,'$.terminal.threadId')=?2 AND json_extract(payload,'$.terminal.turn.id')=?3",
				rusqlite::params![row.work,row.question.thread_id,turn],|r|r.get(0)
			).optional().map_err(error::sqlite_error)?.flatten();
		}

		row.answer = Some(answer);
	}

	Ok(())
}

fn peer(
	c: &Connection,
	event: &AgentInboxEvent,
) -> Result<Option<(PeerExchange, i64)>, StoreError> {
	if !matches!(event.event_kind.as_str(), "collaboration_request" | "collaboration_result") {
		return Ok(None);
	}

	c.query_row(
		"SELECT q.work_id,q.id,i.id,w.owner_agent_id,q.recipient_agent_id,q.instruction_id=i.id,q.state,q.outcome,q.note,i.event_id,(SELECT json_extract(d.payload,'$.threadId') FROM agent_inbox_events d JOIN agent_inbox_events original ON original.id=i.event_id WHERE d.source_event_id=json_array('collaboration_delivery',i.event_id) AND d.work_item_id=q.recipient_agent_id AND d.event_kind='collaboration_delivery' AND json_extract(d.payload,'$.turnId')=original.delivered_turn_id) FROM collaboration_instructions i JOIN collaboration_requests q ON q.id=i.request_id JOIN works w ON w.id=q.work_id WHERE q.id=json_extract(?1,'$.requestId') AND i.id=coalesce(json_extract(?1,'$.instructionId'),CASE WHEN json_extract(?2,'$[0]')='collaboration_declined' THEN json_extract(?2,'$[2]') END)",
		rusqlite::params![event.payload,event.source_event_id],|r|Ok((PeerExchange {work_id:r.get(0)?,request_id:r.get(1)?,instruction_id:r.get(2)?,requesting_agent:r.get(3)?,recipient_agent:r.get(4)?,current:r.get(5)?,state:r.get(6)?,outcome:r.get(7)?,note:r.get(8)?,delivery_thread:r.get(10)?},r.get(9)?))
	).optional().map_err(|e|error::sqlite_error(e).into())
}
