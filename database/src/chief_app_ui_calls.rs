//! Immutable App UI tool attempts. A saved attempt never grants permission to replay.
use crate::{
	ChiefConfigOwner, SqliteStore, StoreError,
	chief_config_journal::{digest, owned, text},
	error::sqlite_error,
	unix_micros,
};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChiefAppUiCallAttempt {
	pub owner: ChiefConfigOwner,
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
pub struct ChiefAppUiCallReceipt {
	pub id: i64,
	pub attempt: ChiefAppUiCallAttempt,
	pub state: String,
	pub result: Option<Value>,
	pub uncertainty_acknowledged: bool,
}

impl SqliteStore {
	/// Call only after explicit confirmation and native source/catalog revalidation.
	/// Only the caller that receives Some(id) can dispatch; all others must read the receipt.
	pub async fn reserve_chief_app_ui_call(
		&self,
		attempt: ChiefAppUiCallAttempt,
	) -> Result<Option<i64>, StoreError> {
		if ![
			&attempt.owner.work,
			&attempt.owner.thread,
			&attempt.owner.generation,
			&attempt.owner.account,
			&attempt.turn,
			&attempt.item,
			&attempt.server,
			&attempt.tool,
			&attempt.attempt_id,
		]
		.iter()
		.all(|v| text(v))
			|| attempt.attempt_id.len() > 512
			|| !digest(&attempt.source_fingerprint)
			|| !digest(&attempt.review_token)
			|| !attempt.arguments.is_object()
		{
			return Err(StoreError::InvalidInput("invalid App UI call"));
		}
		let payload = serde_json::to_string(&attempt)
			.map_err(|_| StoreError::InvalidInput("invalid App UI call"))?;
		if payload.len() > decodex_core::MAX_NATIVE_MESSAGE_BYTES {
			return Err(StoreError::InvalidInput("App UI call exceeds capacity"));
		}
		self.run(move|c|{
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
            if !owned(&tx,&attempt.owner)? {return Err(StoreError::OwnershipLost("App UI call owner"));}
            let blocked:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events a LEFT JOIN chief_inbox_events r ON r.source_event_id=a.source_event_id||':result' LEFT JOIN chief_inbox_events k ON k.source_event_id=a.source_event_id||':ack' WHERE a.event_kind='app_ui_tool_attempt' AND a.work_item_id=?1 AND ((COALESCE(r.disposition_note,'reserved') IN ('reserved','unknown') AND k.id IS NULL) OR json_extract(a.payload,'$.attempt_id')=?2 OR json_extract(a.payload,'$.review_token')=?3))",params![attempt.owner.work,attempt.attempt_id,attempt.review_token],|r|r.get(0)).map_err(sqlite_error)?;
            if blocked {return Ok(None);}
            let now=unix_micros()?;
            let source=format!("app-ui-tool:{}",attempt.attempt_id);
            let summary=json!({"owner":attempt.owner,"turn":attempt.turn,"item":attempt.item,"server":attempt.server,"tool":attempt.tool,"attempt_id":attempt.attempt_id,"review_token":attempt.review_token,"detailsStored":true}).to_string();
            let inserted=tx.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'app_ui_tool_attempt',?3,?4,'resolved','reserved',?4)",params![source,attempt.owner.work,summary,now]).map_err(sqlite_error)?;
            if inserted!=1 {return Ok(None);}
            let id=tx.last_insert_rowid();
            tx.execute("INSERT INTO chief_request_payloads(event_id,payload) VALUES(?1,?2)",params![id,payload]).map_err(sqlite_error)?;
            tx.commit().map_err(sqlite_error)?;
            Ok(Some(id))
        }).await
	}

	/// Retain a result once. Unknown preserves ambiguity; unsent requires positive local evidence.
	pub async fn finish_chief_app_ui_call(
		&self,
		id: i64,
		attempt_id: String,
		state: String,
		result: Option<Value>,
	) -> Result<bool, StoreError> {
		if !matches!(state.as_str(), "completed" | "unknown" | "unsent")
			|| (state == "completed") != result.is_some()
			|| result.as_ref().is_some_and(|result| {
				!result.is_object()
					|| !result["content"].is_array()
					|| (!result["isError"].is_null() && !result["isError"].is_boolean())
			}) {
			return Err(StoreError::InvalidInput("invalid App UI call result"));
		}
		let payload =
			serde_json::to_string(&json!({"result":result})).expect("JSON result serializes");
		if payload.len() > decodex_core::MAX_NATIVE_MESSAGE_BYTES {
			return Err(StoreError::InvalidInput("App UI result exceeds capacity"));
		}
		self.run(move|c|{
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let now=unix_micros()?;
            let inserted=tx.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) SELECT source_event_id||':result',work_item_id,'app_ui_tool_result','{}',?4,'resolved',?3,?4 FROM chief_inbox_events WHERE id=?1 AND event_kind='app_ui_tool_attempt' AND json_extract(payload,'$.attempt_id')=?2",params![id,attempt_id,state,now]).map_err(sqlite_error)?;
            if inserted!=1 {return Ok(false);}
            tx.execute("INSERT INTO chief_request_payloads(event_id,payload) VALUES(?1,?2)",params![tx.last_insert_rowid(),payload]).map_err(sqlite_error)?;
            tx.commit().map_err(sqlite_error)?; Ok(true)
        }).await
	}

	pub async fn chief_app_ui_call_receipt(
		&self,
		work: String,
		attempt_id: String,
	) -> Result<Option<ChiefAppUiCallReceipt>, StoreError> {
		self.run(move|c|{
            let row:Option<(i64,String,String,Option<String>,bool)>=c.query_row("SELECT a.id,p.payload,COALESCE(r.disposition_note,'reserved'),o.payload,k.id IS NOT NULL FROM chief_inbox_events a JOIN chief_request_payloads p ON p.event_id=a.id LEFT JOIN chief_inbox_events r ON r.source_event_id=a.source_event_id||':result' LEFT JOIN chief_request_payloads o ON o.event_id=r.id LEFT JOIN chief_inbox_events k ON k.source_event_id=a.source_event_id||':ack' WHERE a.work_item_id=?1 AND a.event_kind='app_ui_tool_attempt' AND json_extract(a.payload,'$.attempt_id')=?2",params![work,attempt_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(sqlite_error)?;
            row.map(|(id,payload,state,result,ack)|{
                let attempt=serde_json::from_str(&payload).map_err(|_|StoreError::InvalidInput("invalid saved App UI call"))?;
                let result=result.map(|text|serde_json::from_str::<Value>(&text).map(|value|value["result"].clone())).transpose().map_err(|_|StoreError::InvalidInput("invalid saved App UI result"))?.filter(|v|!v.is_null());
                Ok(ChiefAppUiCallReceipt{id,attempt,state,result,uncertainty_acknowledged:ack})
            }).transpose()
        }).await
	}

	/// Find unresolved evidence after a view or service restart, without replaying it.
	pub async fn pending_chief_app_ui_call(
		&self,
		work: String,
	) -> Result<Option<ChiefAppUiCallReceipt>, StoreError> {
		let owner = work.clone();
		let attempt:Option<String>=self.run(move|c| c.query_row("SELECT json_extract(a.payload,'$.attempt_id') FROM chief_inbox_events a LEFT JOIN chief_inbox_events r ON r.source_event_id=a.source_event_id||':result' LEFT JOIN chief_inbox_events k ON k.source_event_id=a.source_event_id||':ack' WHERE a.event_kind='app_ui_tool_attempt' AND a.work_item_id=?1 AND COALESCE(r.disposition_note,'reserved') IN ('reserved','unknown') AND k.id IS NULL ORDER BY a.id DESC LIMIT 1",[owner],|r|r.get(0)).optional().map_err(|e|sqlite_error(e).into())).await?;
		match attempt {
			Some(attempt) => self.chief_app_ui_call_receipt(work, attempt).await,
			None => Ok(None),
		}
	}

	/// An explicit user acknowledgment permits later, separately confirmed calls. Never replay this
	/// call.
	pub async fn acknowledge_chief_app_ui_uncertainty(
		&self,
		work: String,
		id: i64,
		attempt_id: String,
	) -> Result<bool, StoreError> {
		self.run(move|c|{
            let now=unix_micros()?;
            Ok(c.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) SELECT a.source_event_id||':ack',a.work_item_id,'app_ui_tool_ack','{}',?4,'resolved','acknowledged',?4 FROM chief_inbox_events a JOIN chief_inbox_events r ON r.source_event_id=a.source_event_id||':result' WHERE a.id=?1 AND a.work_item_id=?2 AND a.event_kind='app_ui_tool_attempt' AND json_extract(a.payload,'$.attempt_id')=?3 AND r.disposition_note='unknown'",params![id,work,attempt_id,now]).map_err(sqlite_error)?==1)
        }).await
	}
}
