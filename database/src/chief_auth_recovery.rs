//! Historical provider sign-in observations, never execution or account authority.

use crate::{SqliteStore, StoreError, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

pub struct ChiefAuthRecoveryObservation {
	pub thread_id: String,
	pub turn_id: String,
	pub provider: String,
	pub message: String,
	pub completed: bool,
	pub connection_id: String,
	pub generation_id: Option<String>,
}

impl SqliteStore {
	/// Append a receipt for the exact running turn and current native owner.
	/// Events have no native identity: repeated receipts do not imply distinct attempts.
	pub async fn record_chief_auth_recovery(
		&self,
		observation: ChiefAuthRecoveryObservation,
	) -> Result<bool, StoreError> {
		let o = observation;
		if [&o.thread_id, &o.turn_id, &o.provider, &o.connection_id]
			.iter()
			.any(|value| value.is_empty() || value.len() > 512)
			|| o.message.is_empty()
			|| o.message.len() > 4096
		{
			return Err(StoreError::InvalidInput("invalid authentication recovery observation"));
		}
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work: Option<String> = tx.query_row("SELECT id FROM chief_work_items WHERE codex_thread_id=?1 AND active_turn_id=?2 AND dispatch_state='running'",params![o.thread_id,o.turn_id],|row|row.get(0)).optional().map_err(sqlite_error)?;
			let Some(work) = work else { return Ok(false); };
			if !crate::chief_process::owns_work(&tx,&work,o.generation_id.as_deref())? { return Ok(false); }
			let account: Option<String> = if let Some(generation) = &o.generation_id {
				tx.query_row("SELECT account_id FROM chief_process_bindings WHERE generation_id=?1",[generation],|row|row.get(0)).optional().map_err(sqlite_error)?
			} else { None };
			let kind = if o.completed { "auth_recovery_completed" } else { "auth_recovery_started" };
			let payload = serde_json::json!({"threadId":o.thread_id,"turnId":o.turn_id,"provider":o.provider,"message":o.message,"connectionId":o.connection_id,"generationId":o.generation_id,"accountId":account}).to_string();
			let now = unix_micros()?;
			// Allocate receipt identity under the same write transaction. No attempt ID
			// exists upstream, so do not collapse subsequent recovery cycles in a turn.
			tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(json_array('auth_recovery_receipt',?1,(SELECT coalesce(max(id),0)+1 FROM chief_inbox_events)),?1,?2,?3,?4,'resolved','Provider sign-in event observed; work judgment unchanged.',?4,?1,?5)",params![work,kind,payload,now,o.turn_id]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(true)
		}).await
	}
}
