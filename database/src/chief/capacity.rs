//! Durable, bounded retries for confirmed model-capacity failures.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefCapacityRetry {
	pub event_id: i64,
	pub work_item_id: String,
	pub failed_turn_id: String,
	pub attempt: i64,
	pub due_at_micros: i64,
}

fn retry_row(row: &Row<'_>) -> rusqlite::Result<ChiefCapacityRetry> {
	Ok(ChiefCapacityRetry {
		event_id: row.get("event_id")?,
		work_item_id: row.get("work_item_id")?,
		failed_turn_id: row.get("failed_turn_id")?,
		attempt: row.get("attempt")?,
		due_at_micros: row.get("due_at_micros")?,
	})
}

pub(super) fn next_retry(
	connection: &Connection,
	work: &str,
	turn: &str,
	now: i64,
) -> Result<Option<(i64, i64)>, StoreError> {
	let previous: Option<i64> = connection.query_row("SELECT attempt FROM chief_capacity_retries WHERE work_item_id=?1 AND retry_turn_id=?2 AND state='submitted'",params![work,turn],|row| row.get(0)).optional().map_err(sqlite_error)?;
	let attempt = previous.unwrap_or(0) + 1;
	let delay = match attempt {
		1 => 15_000_000,
		2 => 30_000_000,
		3 => 60_000_000,
		_ => return Ok(None),
	};
	Ok(Some((
		attempt,
		now.checked_add(delay).ok_or(StoreError::InvalidInput("retry deadline overflow"))?,
	)))
}

pub(super) fn cancel_pending(connection: &Connection, work: &str) -> Result<(), StoreError> {
	connection.execute("UPDATE chief_inbox_events SET disposition='resolved', disposition_note='Automatic retry cancelled or superseded by new input.', disposed_at_micros=max(created_at_micros,?2) WHERE disposition IS NULL AND id IN (SELECT event_id FROM chief_capacity_retries WHERE work_item_id=?1 AND state='pending')",params![work,unix_micros()?]).map_err(sqlite_error)?;
	connection
		.execute(
			"UPDATE chief_capacity_retries SET state='cancelled' WHERE work_item_id=?1 AND state='pending'",
			[work],
		)
		.map_err(sqlite_error)?;
	Ok(())
}

impl SqliteStore {
	pub async fn pending_chief_capacity_retry(
		&self,
		work: String,
	) -> Result<Option<ChiefCapacityRetry>, StoreError> {
		self.run(move |connection| {
			connection
				.query_row(
					"SELECT * FROM chief_capacity_retries WHERE work_item_id=?1 AND state='pending'",
					[work],
					retry_row,
				)
				.optional()
				.map_err(|e| sqlite_error(e).into())
		})
		.await
	}

	pub async fn due_chief_capacity_retries(
		&self,
		now: i64,
	) -> Result<Vec<ChiefCapacityRetry>, StoreError> {
		self.run(move |connection| connection.prepare("SELECT r.* FROM chief_capacity_retries r JOIN chief_work_items w ON w.id=r.work_item_id WHERE r.state='pending' AND r.due_at_micros<=?1 AND w.dispatch_state='idle' ORDER BY r.due_at_micros,r.event_id LIMIT 100").map_err(sqlite_error)?.query_map([now],retry_row).map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(|e|sqlite_error(e).into())).await
	}

	/// Claim and fence before the external turn request. A crash never permits replay.
	pub async fn begin_chief_capacity_retry(
		&self,
		work: String,
		event: i64,
		now: i64,
	) -> Result<(), StoreError> {
		self.run(move |connection| {
            let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let item=read_work(&tx,&work)?;
            if item.dispatch_state!=ChiefDispatchState::Idle || item.codex_thread_id.is_none() { return Err(DatabaseError::Conflict.into()); }
            let retry=tx.query_row("SELECT * FROM chief_capacity_retries WHERE event_id=?1 AND work_item_id=?2 AND state='pending' AND due_at_micros<=?3",params![event,work,now],retry_row).optional().map_err(sqlite_error)?.ok_or(DatabaseError::Conflict)?;
            tx.execute("UPDATE chief_capacity_retries SET state='claimed' WHERE event_id=?1",[event]).map_err(sqlite_error)?;
            tx.execute("UPDATE chief_inbox_events SET disposition='resolved', disposition_note='Automatic capacity retry requested.', disposed_at_micros=max(created_at_micros,?2) WHERE id=?1 AND disposition IS NULL",params![event,unix_micros()?]).map_err(sqlite_error)?;
            tx.execute("UPDATE chief_work_items SET dispatch_state='dispatching',updated_at_micros=max(updated_at_micros,?2) WHERE id=?1",params![work,unix_micros()?]).map_err(sqlite_error)?;
            // Carry delivery receipts forward, without copying user input into another prompt.
            tx.execute("UPDATE chief_inbox_events SET delivered_turn_id='' WHERE delivery_work_item_id=?1 AND delivered_turn_id=?2 AND disposition IS NULL",params![work,retry.failed_turn_id]).map_err(sqlite_error)?;
            tx.commit().map_err(sqlite_error)?; Ok(())
        }).await
	}

	pub async fn cancel_chief_capacity_retry(
		&self,
		work: String,
		event: i64,
	) -> Result<(), StoreError> {
		self.run(move |connection| {
            let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let found: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_capacity_retries WHERE event_id=?1 AND work_item_id=?2 AND state='pending')",params![event,work],|r|r.get(0)).map_err(sqlite_error)?;
            if !found { return Err(DatabaseError::Conflict.into()); }
            cancel_pending(&tx,&work)?;
            let item=read_work(&tx,&work)?;
            if item.parent_goal_id.is_some() {
                let original=read_event(&tx,event)?;
                let mut payload:serde_json::Value=serde_json::from_str(&original.payload).map_err(|_|StoreError::InvalidInput("invalid capacity receipt"))?;
                payload["capacityRetry"]["cancelled"]=serde_json::json!(true);
                tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES (?1,?2,'worker_turn_completed',?3,?4)",params![format!("capacity-cancel:{event}"),work,payload.to_string(),unix_micros()?]).map_err(sqlite_error)?;
            }
            tx.commit().map_err(sqlite_error)?; Ok(())
        }).await
	}
}
