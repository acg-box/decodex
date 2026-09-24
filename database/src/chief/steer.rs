//! Resolve uncertain steering only from the exact native submission receipt.
use super::{bounded, read_event};
use crate::{DatabaseError, SqliteStore, StoreError, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, Transaction, TransactionBehavior, params};

impl SqliteStore {
	/// Read a positive receipt without changing or replaying the submission.
	pub async fn chief_steer_confirmed(
		&self,
		work: String,
		thread: String,
		turn: String,
		key: String,
	) -> Result<bool, StoreError> {
		for id in [&work, &thread, &turn, &key] {
			bounded(id, 512)?;
		}
		self.run(move |connection| {
			connection.query_row(
				"SELECT EXISTS(SELECT 1 FROM chief_inbox_events e JOIN chief_work_items w ON w.id=e.work_item_id JOIN chief_inbox_events r ON r.source_event_id=json_array('steer_receipt',e.id) AND r.work_item_id=w.id AND r.event_kind='user_message' AND r.delivered_turn_id=e.delivered_turn_id WHERE w.id=?1 AND w.codex_thread_id=?2 AND e.source_event_id=json_array('user_steer',w.id,?4) AND e.event_kind='steer_pending' AND e.disposition='resolved' AND e.delivered_turn_id=?3 AND json_extract(e.payload,'$.threadId')=?2)",
				params![work,thread,turn,key], |row| row.get(0),
			).map_err(|error| sqlite_error(error).into())
		}).await
	}

	/// A native user-message receipt proves consumption of one saved submission.
	/// ID-less or unrelated receipts never resolve an uncertain attempt.
	pub async fn observe_chief_steer_receipt(
		&self,
		thread: String,
		turn: String,
		client_id: String,
		generation: Option<String>,
	) -> Result<(), StoreError> {
		for id in [&thread, &turn, &client_id] {
			bounded(id, 512)?;
		}
		self.run(move |connection| {
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let candidate: Option<(i64, String)> = tx.query_row(
                "SELECT e.id,w.id FROM chief_inbox_events e JOIN chief_work_items w ON w.id=e.work_item_id WHERE w.codex_thread_id=?1 AND e.event_kind='steer_pending' AND e.disposition IS NULL AND e.delivery_work_item_id=w.id AND e.delivered_turn_id=?2 AND json_extract(e.payload,'$.threadId')=?1 AND e.source_event_id=json_array('user_steer',w.id,?3)",
                params![thread, turn, client_id], |row| Ok((row.get(0)?,row.get(1)?)),
            ).optional().map_err(sqlite_error)?;
            if let Some((event, work)) = candidate
                && crate::chief_process::owns_work(&tx, &work, generation.as_deref())? {
                finish(&tx, event, true)?;
            }
            tx.commit().map_err(sqlite_error)?;
            Ok(())
        }).await
	}
}

pub(super) fn finish(tx: &Transaction<'_>, event: i64, accepted: bool) -> Result<(), StoreError> {
	let attempt = read_event(tx, event)?;
	if attempt.event_kind != "steer_pending" {
		return Err(DatabaseError::Conflict.into());
	}
	if attempt.disposition.is_some() {
		let receipt = serde_json::json!(["steer_receipt", event]).to_string();
		let confirmed: bool = tx.query_row(
			"SELECT EXISTS(SELECT 1 FROM chief_inbox_events WHERE source_event_id=?1 AND event_kind='user_message')",
			[receipt], |row| row.get(0),
		).map_err(sqlite_error)?;
		return if accepted && confirmed { Ok(()) } else { Err(DatabaseError::Conflict.into()) };
	}
	let note = if accepted {
		"Steer accepted by the provider."
	} else {
		"Steer rejected by the provider; no input was queued."
	};
	let now = unix_micros()?.max(attempt.created_at_micros);
	tx.execute("UPDATE chief_inbox_events SET disposition='resolved',disposition_note=?2,disposed_at_micros=?3 WHERE id=?1",params![event,note,now]).map_err(sqlite_error)?;
	if accepted {
		let source = serde_json::json!(["steer_receipt", event]).to_string();
		crate::chief_questions::retire_for_prompt(tx, &attempt.work_item_id, &attempt.payload)?;
		tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,?2,'user_message',?3,?4,?2,?5)",params![source,attempt.work_item_id,attempt.payload,now,attempt.delivered_turn_id]).map_err(sqlite_error)?;
	}
	Ok(())
}
