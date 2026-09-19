//! Durable provider precaution; a normal prompt never clears this state.
use crate::{SqliteStore, StoreError, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, params};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChiefMisalignment {
	pub thread_id: String,
	pub turn_id: String,
	pub details_json: Option<String>,
}

impl ChiefMisalignment {
	pub fn review_id(&self) -> String {
		use sha2::{Digest as _, Sha256};
		let identity =
			serde_json::json!([self.thread_id, self.turn_id, self.details_json]).to_string();
		Sha256::digest(identity.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
	}
}

impl SqliteStore {
	/// Claim only the exact reviewed findings. Keep the precaution until a new turn is
	/// acknowledged.
	pub async fn begin_chief_misalignment_continuation(
		&self,
		work: String,
		expected: ChiefMisalignment,
		key: String,
	) -> Result<i64, StoreError> {
		if key.is_empty() || key.len() > 512 {
			return Err(StoreError::InvalidInput("invalid continuation key"));
		}
		let details = expected
			.details_json
			.as_ref()
			.ok_or(StoreError::InvalidInput("no findings to acknowledge"))?;
		let value: serde_json::Value = serde_json::from_str(details)
			.map_err(|_| StoreError::InvalidInput("invalid findings"))?;
		let text = value
			.pointer("/steer/message")
			.and_then(serde_json::Value::as_str)
			.filter(|text| !text.trim().is_empty() && text.len() <= 1024)
			.ok_or(StoreError::InvalidInput("no valid continuation text"))?;
		if !value["detailedExplanation"]
			.as_str()
			.is_some_and(|text| !text.trim().is_empty() && text.len() <= 65536)
		{
			return Err(StoreError::InvalidInput("no valid findings"));
		}
		let payload=serde_json::json!({"threadId":expected.thread_id,"turnId":expected.turn_id,"text":text}).to_string();
		self.run(move |connection| {
            let tx=connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let changed=tx.execute("UPDATE chief_work_items SET dispatch_state='dispatching',updated_at_micros=max(updated_at_micros,?5) WHERE id=?1 AND codex_thread_id=?2 AND dispatch_state='idle' AND EXISTS(SELECT 1 FROM chief_misalignment WHERE work_id=?1 AND thread_id=?2 AND turn_id=?3 AND details_json IS ?4)",params![work,expected.thread_id,expected.turn_id,expected.details_json,unix_micros()?]).map_err(sqlite_error)?;
            if changed!=1 { return Err(crate::DatabaseError::Conflict.into()); }
            let source=serde_json::json!(["misalignment_continuation",work,key]).to_string();
            tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES(?1,?2,'misalignment_continuation_pending',?3,?4)",params![source,work,payload,unix_micros()?]).map_err(sqlite_error)?;
            let event=tx.last_insert_rowid();
            tx.commit().map_err(sqlite_error)?;
            Ok(event)
        }).await
	}

	/// A missing response never calls this method. Rejection keeps the precaution.
	pub async fn finish_chief_misalignment_continuation(
		&self,
		work: String,
		event: i64,
		expected: ChiefMisalignment,
		accepted_turn: Option<String>,
	) -> Result<(), StoreError> {
		if accepted_turn
			.as_ref()
			.is_some_and(|turn| turn.is_empty() || turn.len() > 512 || turn == &expected.turn_id)
		{
			return Err(StoreError::InvalidInput("invalid continuation turn"));
		}
		self.run(move |connection| {
            let tx=connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let current:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_work_items w JOIN chief_misalignment m ON m.work_id=w.id WHERE w.id=?1 AND w.codex_thread_id=?2 AND w.dispatch_state='dispatching' AND m.thread_id=?2 AND m.turn_id=?3 AND m.details_json IS ?4)",params![work,expected.thread_id,expected.turn_id,expected.details_json],|row|row.get(0)).map_err(sqlite_error)?;
            if !current { return Err(crate::DatabaseError::Conflict.into()); }
            let changed=tx.execute("UPDATE chief_inbox_events SET disposition='resolved',disposition_note=?3,disposed_at_micros=max(created_at_micros,?4) WHERE id=?1 AND work_item_id=?2 AND event_kind='misalignment_continuation_pending' AND disposition IS NULL",params![event,work,if accepted_turn.is_some(){"Explicit continuation acknowledged."}else{"Continuation rejected; precaution retained."},unix_micros()?]).map_err(sqlite_error)?;
            if changed!=1 { return Err(crate::DatabaseError::Conflict.into()); }
            tx.execute("UPDATE chief_work_items SET dispatch_state=?2,active_turn_id=?3,updated_at_micros=max(updated_at_micros,?4) WHERE id=?1",params![work,if accepted_turn.is_some(){"running"}else{"idle"},accepted_turn,unix_micros()?]).map_err(sqlite_error)?;
            if accepted_turn.is_some() { tx.execute("DELETE FROM chief_misalignment WHERE work_id=?1",[work]).map_err(sqlite_error)?; }
            tx.commit().map_err(sqlite_error)?;
            Ok(())
        }).await
	}

	pub async fn record_chief_misalignment(
		&self,
		thread: String,
		turn: String,
		details: Option<String>,
	) -> Result<(), StoreError> {
		self.record_misalignment_source(thread, turn, details, false).await
	}

	/// Restore an idle thread only after the caller verifies its latest native failed turn.
	pub async fn restore_chief_misalignment(
		&self,
		thread: String,
		turn: String,
		details: Option<String>,
	) -> Result<(), StoreError> {
		self.record_misalignment_source(thread, turn, details, true).await
	}

	async fn record_misalignment_source(
		&self,
		thread: String,
		turn: String,
		details: Option<String>,
		restore_idle: bool,
	) -> Result<(), StoreError> {
		if thread.is_empty()
			|| thread.len() > 512
			|| turn.is_empty()
			|| turn.len() > 512
			|| details.as_ref().is_some_and(|value| {
				value.len() > 400_000
					|| !serde_json::from_str::<serde_json::Value>(value)
						.is_ok_and(|value| value.is_object())
			}) {
			return Err(StoreError::InvalidInput("invalid misalignment evidence"));
		}
		self.run(move |connection| {
            let tx=connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let work:Option<String>=tx.query_row("SELECT id FROM chief_work_items WHERE codex_thread_id=?1 AND (active_turn_id=?2 OR (?3 AND dispatch_state='idle' AND active_turn_id IS NULL) OR (active_turn_id IS NULL AND EXISTS(SELECT 1 FROM chief_misalignment m WHERE m.work_id=chief_work_items.id AND m.thread_id=?1 AND m.turn_id=?2)))",params![thread,turn,restore_idle],|row|row.get(0)).optional().map_err(sqlite_error)?;
            if let Some(work)=work {
                tx.execute("INSERT INTO chief_misalignment VALUES(?1,?2,?3,?4,?5) ON CONFLICT(work_id) DO UPDATE SET thread_id=excluded.thread_id,turn_id=excluded.turn_id,details_json=CASE WHEN chief_misalignment.thread_id=excluded.thread_id AND chief_misalignment.turn_id=excluded.turn_id THEN coalesce(excluded.details_json,chief_misalignment.details_json) ELSE excluded.details_json END,created_at_micros=excluded.created_at_micros",params![work,thread,turn,details,unix_micros()?]).map_err(sqlite_error)?;
                tx.execute("UPDATE chief_inbox_events SET disposition='resolved',disposition_note='Input retired without delivery because the provider paused this conversation.',disposed_at_micros=max(created_at_micros,?2) WHERE work_item_id=?1 AND event_kind IN ('user_message','async_question_answer') AND disposition IS NULL AND delivered_turn_id IS NULL",params![work,unix_micros()?]).map_err(sqlite_error)?;
                tx.execute("UPDATE chief_capacity_retries SET state='cancelled' WHERE work_item_id=?1 AND state='pending'",[&work]).map_err(sqlite_error)?;

            }
            tx.commit().map_err(sqlite_error)?;
            Ok(())
        }).await
	}

	pub async fn chief_misalignment(
		&self,
		work: String,
	) -> Result<Option<ChiefMisalignment>, StoreError> {
		self.run(move |connection| {
            connection.query_row("SELECT m.thread_id,m.turn_id,m.details_json FROM chief_misalignment m JOIN chief_work_items w ON w.id=m.work_id AND w.codex_thread_id=m.thread_id WHERE m.work_id=?1",[work],|row|Ok(ChiefMisalignment {thread_id:row.get(0)?,turn_id:row.get(1)?,details_json:row.get(2)?})).optional().map_err(|error|sqlite_error(error).into())
        }).await
	}
}
