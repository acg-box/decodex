//! Native conversation names and automatic-title review watermarks. Agent names stay separate.
use crate::{SqliteStore, StoreError, agent_process, error};
use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentConversationTitle {
	pub name: Option<String>,
	pub automatic: bool,
	pub generated: bool,
	pub reviewed_through: i64,
}

pub struct AgentTitleReview {
	pub state: Option<AgentConversationTitle>,
	pub completed_event: i64,
	pub new_exchanges: i64,
}

fn latest(
	connection: &Connection,
	work: &str,
	thread: &str,
) -> Result<Option<(i64, AgentConversationTitle)>, StoreError> {
	let row: Option<(i64, String)> = connection.query_row("SELECT id,json_extract(payload,'$.title') FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind='conversation_title' AND json_extract(payload,'$.threadId')=?2 ORDER BY id DESC LIMIT 1",rusqlite::params![work,thread], |r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(error::sqlite_error)?;
	row.map(|(id, json)| {
		serde_json::from_str(&json)
			.map(|state| (id, state))
			.map_err(|_| StoreError::InvalidInput("invalid conversation title observation"))
	})
	.transpose()
}

pub(crate) fn titles(connection: &Connection) -> Result<BTreeMap<String, String>, StoreError> {
	connection.prepare("SELECT w.codex_thread_id,json_extract(e.payload,'$.title.name') FROM agent_work_items w JOIN agent_inbox_events e ON e.id=(SELECT id FROM agent_inbox_events WHERE work_item_id=w.id AND event_kind='conversation_title' AND json_extract(payload,'$.threadId')=w.codex_thread_id ORDER BY id DESC LIMIT 1) WHERE json_extract(e.payload,'$.title.generated')=1 AND json_extract(e.payload,'$.title.name') IS NOT NULL").map_err(error::sqlite_error)?.query_map([],|r|Ok((r.get(0)?,r.get(1)?))).map_err(error::sqlite_error)?.collect::<Result<BTreeMap<_,_>,_>>().map_err(|error|error::sqlite_error(error).into())
}

impl SqliteStore {
	pub async fn agent_conversation_title(
		&self,
		work: String,
		thread: String,
	) -> Result<Option<AgentConversationTitle>, StoreError> {
		self.run(move |c| latest(c, &work, &thread).map(|v| v.map(|(_, state)| state))).await
	}

	/// Read the review checkpoint and count completed user exchanges since that checkpoint.
	pub async fn agent_title_review(
		&self,
		work: String,
		thread: String,
		turn: String,
	) -> Result<Option<AgentTitleReview>, StoreError> {
		self.run(move |connection| {
			let terminal: Option<i64> = connection.query_row("SELECT e.id FROM agent_work_items w JOIN agent_inbox_events e ON e.work_item_id=w.id WHERE w.id=?1 AND w.codex_thread_id=?2 AND w.active_turn_id IS NULL AND w.dispatch_state='idle' AND e.source_event_id=json_array('turn/completed',?2,?3) AND e.event_kind='agent_turn_completed' AND json_extract(e.payload,'$.terminal.turn.status')='completed' AND EXISTS(SELECT 1 FROM agent_inbox_events u WHERE u.work_item_id=w.id AND u.event_kind IN ('user_message','async_question_answer') AND u.delivery_work_item_id=w.id AND u.delivered_turn_id=?3)",rusqlite::params![work,thread,turn],|r|r.get(0)).optional().map_err(error::sqlite_error)?;
			let Some(completed_event)=terminal else { return Ok(None); };
			let state=latest(connection,&work,&thread)?.map(|(_,state)|state);
			let through=state.as_ref().map_or(0,|s|s.reviewed_through);
			let new_exchanges=connection.query_row("SELECT count(*) FROM agent_inbox_events e WHERE e.work_item_id=?1 AND e.event_kind='agent_turn_completed' AND e.id>?3 AND e.id<=?4 AND json_extract(e.payload,'$.terminal.threadId')=?2 AND json_extract(e.payload,'$.terminal.turn.status')='completed' AND EXISTS(SELECT 1 FROM agent_inbox_events u WHERE u.work_item_id=?1 AND u.event_kind IN ('user_message','async_question_answer') AND u.delivery_work_item_id=?1 AND u.delivered_turn_id=json_extract(e.payload,'$.terminal.turn.id'))",rusqlite::params![work,thread,through,completed_event],|r|r.get(0)).map_err(error::sqlite_error)?;
			Ok(Some(AgentTitleReview {state,completed_event,new_exchanges}))
		}).await
	}

	/// Save a non-waking name observation for an exact currently owned conversation.
	/// A seed records an inherited/bootstrap name only when no prior title observation exists.
	pub async fn observe_agent_title(
		&self,
		work: String,
		thread: String,
		generation: Option<String>,
		state: AgentConversationTitle,
		seed: bool,
	) -> Result<bool, StoreError> {
		if state.reviewed_through < 0
			|| state.name.as_ref().is_some_and(|s| {
				s.trim().is_empty()
					|| s.len() > 1024
					|| s.chars().any(char::is_control)
					|| decodex_core::contains_credential_material(s)
			}) {
			return Err(StoreError::InvalidInput("invalid conversation title"));
		}
		self.run(move |connection| {
			let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let bound:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_work_items WHERE id=?1 AND codex_thread_id=?2)",rusqlite::params![work,thread],|r|r.get(0)).map_err(error::sqlite_error)?;
			if !bound || !agent_process::owns_work(&tx,&work,generation.as_deref())? { return Ok(false); }
			let previous=latest(&tx,&work,&thread)?;
			if previous.as_ref().is_some_and(|(_,old)|seed || old==&state || old.reviewed_through>state.reviewed_through) { return Ok(false); }
			let payload=serde_json::json!({"threadId":thread,"title":state});
			let key=serde_json::json!(["conversation_title",work,thread,previous.map(|(id,_)|id)]).to_string();
			let now=crate::unix_micros()?;
			tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'conversation_title',?3,?4,'resolved','Native conversation title observed',?4)",rusqlite::params![key,work,payload.to_string(),now]).map_err(error::sqlite_error)?;
			tx.commit().map_err(error::sqlite_error)?;
			Ok(true)
		}).await
	}
}

#[cfg(test)] mod tests;
