//! Historical App UI tool receipts. Reads and recovery never replay a saved attempt.
use crate::{AgentConfigOwner, SqliteStore, StoreError, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentAppUiCallAttempt {
	pub owner: AgentConfigOwner,
	pub turn: String,
	pub item: String,
	pub server: String,
	pub tool: String,
	pub arguments: Value,
	pub source_fingerprint: String,
	pub review_token: String,
	pub attempt_id: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentAppUiCallReceipt {
	pub id: i64,
	pub attempt: AgentAppUiCallAttempt,
	pub state: String,
	pub result: Option<Value>,
	pub uncertainty_acknowledged: bool,
}

impl SqliteStore {
	/// Save uncertainty once after the original process is proved dead.
	async fn mark_agent_app_ui_call_unknown(
		&self,
		id: i64,
		attempt_id: String,
	) -> Result<bool, StoreError> {
		let payload = r#"{"result":null}"#;

		self.run(move|c|{
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let now=unix_micros()?;
            let inserted=tx.execute("INSERT OR IGNORE INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) SELECT source_event_id||':result',work_item_id,'app_ui_tool_result','{}',?4,'resolved',?3,?4 FROM agent_inbox_events WHERE id=?1 AND event_kind='app_ui_tool_attempt' AND json_extract(payload,'$.attempt_id')=?2",params![id,attempt_id,"unknown",now]).map_err(sqlite_error)?;

            if inserted!=1 {return Ok(false);}

            tx.execute("INSERT INTO agent_request_payloads(event_id,payload) VALUES(?1,?2)",params![tx.last_insert_rowid(),payload]).map_err(sqlite_error)?;

            tx.commit().map_err(sqlite_error)?; Ok(true)
        }).await
	}

	pub async fn agent_app_ui_call_receipt(
		&self,
		work: String,
		attempt_id: String,
	) -> Result<Option<AgentAppUiCallReceipt>, StoreError> {
		self.run(move|c|{
            let row:Option<(i64,String,String,Option<String>,bool)>=c.query_row("SELECT a.id,p.payload,COALESCE(r.disposition_note,'reserved'),o.payload,k.id IS NOT NULL FROM agent_inbox_events a JOIN agent_request_payloads p ON p.event_id=a.id LEFT JOIN agent_inbox_events r ON r.source_event_id=a.source_event_id||':result' LEFT JOIN agent_request_payloads o ON o.event_id=r.id LEFT JOIN agent_inbox_events k ON k.source_event_id=a.source_event_id||':ack' WHERE a.work_item_id=?1 AND a.event_kind='app_ui_tool_attempt' AND json_extract(a.payload,'$.attempt_id')=?2",params![work,attempt_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(sqlite_error)?;

            row.map(|(id,payload,state,result,ack)|{
                let attempt=serde_json::from_str(&payload).map_err(|_|StoreError::InvalidInput("invalid saved App UI call"))?;
                let result=result.map(|text|serde_json::from_str::<Value>(&text).map(|value|value["result"].clone())).transpose().map_err(|_|StoreError::InvalidInput("invalid saved App UI result"))?.filter(|v|!v.is_null());

                Ok(AgentAppUiCallReceipt{id,attempt,state,result,uncertainty_acknowledged:ack})
            }).transpose()
        }).await
	}

	/// Find unresolved evidence after a view or service restart, without replaying it.
	pub async fn pending_agent_app_ui_call(
		&self,
		work: String,
	) -> Result<Option<AgentAppUiCallReceipt>, StoreError> {
		let owner = work.clone();
		let attempt:Option<String>=self.run(move|c| c.query_row("SELECT json_extract(a.payload,'$.attempt_id') FROM agent_inbox_events a LEFT JOIN agent_inbox_events r ON r.source_event_id=a.source_event_id||':result' LEFT JOIN agent_inbox_events k ON k.source_event_id=a.source_event_id||':ack' WHERE a.event_kind='app_ui_tool_attempt' AND a.work_item_id=?1 AND COALESCE(r.disposition_note,'reserved') IN ('reserved','unknown') AND k.id IS NULL ORDER BY a.id DESC LIMIT 1",[owner],|r|r.get(0)).optional().map_err(|e|sqlite_error(e).into())).await?;

		match attempt {
			Some(attempt) => self.agent_app_ui_call_receipt(work, attempt).await,
			None => Ok(None),
		}
	}

	/// A dead original process makes an unfinished reservation uncertain, never successful.
	/// Missing process evidence does not settle or replay the call.
	pub async fn recover_agent_app_ui_call(
		&self,
		work: String,
		attempt_id: String,
	) -> Result<bool, StoreError> {
		let operation = attempt_id.clone();
		let id=self.run(move|c|{
			let row:Option<(i64,String)>=c.query_row("SELECT a.id,json_extract(a.payload,'$.owner.generation') FROM agent_inbox_events a WHERE a.work_item_id=?1 AND a.event_kind='app_ui_tool_attempt' AND json_extract(a.payload,'$.attempt_id')=?2 AND NOT EXISTS(SELECT 1 FROM agent_inbox_events r WHERE r.source_event_id=a.source_event_id||':result')",params![work,operation],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sqlite_error)?;

			match row { Some((id,generation)) if crate::agent_config_journal::dead(c,&generation)?=>Ok(Some(id)),_=>Ok(None) }
		}).await?;

		match id {
			Some(id) => self.mark_agent_app_ui_call_unknown(id, attempt_id).await,
			None => Ok(false),
		}
	}

	/// Acknowledge historical uncertainty without changing the outcome or replaying the call.
	pub async fn acknowledge_agent_app_ui_uncertainty(
		&self,
		work: String,
		id: i64,
		attempt_id: String,
	) -> Result<bool, StoreError> {
		self.run(move|c|{
            let now=unix_micros()?;

            Ok(c.execute("INSERT OR IGNORE INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) SELECT a.source_event_id||':ack',a.work_item_id,'app_ui_tool_ack','{}',?4,'resolved','acknowledged',?4 FROM agent_inbox_events a JOIN agent_inbox_events r ON r.source_event_id=a.source_event_id||':result' WHERE a.id=?1 AND a.work_item_id=?2 AND a.event_kind='app_ui_tool_attempt' AND json_extract(a.payload,'$.attempt_id')=?3 AND r.disposition_note='unknown'",params![id,work,attempt_id,now]).map_err(sqlite_error)?==1)
        }).await
	}
}
