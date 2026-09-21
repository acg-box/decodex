//! Observe native-admitted turns without claiming local input delivery.
use crate::{SqliteStore, StoreError, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use sha2::{Digest as _, Sha256};

impl SqliteStore {
	/// Adopt a new native turn only for its exact bound work and current process owner.
	/// The resolved receipt prevents replay after completion or process restart.
	pub async fn observe_chief_native_turn(
		&self,
		thread: String,
		turn: String,
		generation: Option<String>,
		connection_id: String,
	) -> Result<bool, StoreError> {
		if [&thread, &turn, &connection_id].iter().any(|id| id.is_empty() || id.len() > 512) {
			return Err(StoreError::InvalidInput("invalid native turn identity"));
		}
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work: Option<(String,String,Option<String>)> = tx.query_row(
				"SELECT id,dispatch_state,active_turn_id FROM chief_work_items WHERE codex_thread_id=?1",
				[&thread],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))
			).optional().map_err(sqlite_error)?;
			let Some((work,state,active)) = work else {return Ok(false)};
			if !crate::chief_process::owns_work(&tx,&work,generation.as_deref())? {return Ok(false)}
			if state != "idle" && !(state == "running" && active.as_deref()==Some(&turn)) {return Ok(false)}
			let account: Option<String> = if let Some(generation)=&generation {
				tx.query_row("SELECT account_id FROM chief_process_bindings WHERE generation_id=?1",[generation],|row|row.get(0)).optional().map_err(sqlite_error)?
			} else {None};
			let identity=serde_json::json!([work,account,thread,turn]).to_string();
			let digest: String=Sha256::digest(identity.as_bytes()).iter().map(|byte|format!("{byte:02x}")).collect();
			let key=format!("native-turn:{digest}");
			let seen: bool = tx.query_row(
				"SELECT EXISTS(SELECT 1 FROM chief_inbox_events WHERE source_event_id=?1 OR
				(work_item_id=?2 AND event_kind IN ('chief_turn_completed','worker_turn_completed','capacity_retry')
				AND json_valid(payload) AND json_extract(payload,'$.terminal.threadId')=?3
				AND json_extract(payload,'$.terminal.turn.id')=?4))",
				params![key,work,thread,turn],|row|row.get(0)
			).map_err(sqlite_error)?;
			if seen {return Ok(false)}
			let now=unix_micros()?;
			let payload=serde_json::json!({"threadId":thread,"turnId":turn,"accountId":account,"generationId":generation,"connectionId":connection_id}).to_string();
			tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'native_turn_started',?3,?4,'resolved','Native turn observed; no local input delivery inferred.',?4)",params![key,work,payload,now]).map_err(sqlite_error)?;
			if state == "idle" {
				tx.execute("UPDATE chief_work_items SET dispatch_state='running',active_turn_id=?2,updated_at_micros=max(updated_at_micros,?3) WHERE id=?1",params![work,turn,now]).map_err(sqlite_error)?;
				tx.execute("UPDATE chief_usage SET turn_id=?2,baseline_input_tokens=json_extract(usage_json,'$.input_tokens'),baseline_output_tokens=json_extract(usage_json,'$.output_tokens'),turn_input_tokens=NULL,turn_output_tokens=NULL WHERE work_id=?1 AND thread_id=?3",params![work,turn,thread]).map_err(sqlite_error)?;
			}
			tx.commit().map_err(sqlite_error)?;
			Ok(state == "idle")
		}).await
	}
}
