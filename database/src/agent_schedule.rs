//! Independent Agent duties share the Assistant's durable journal and due-check tick.
use decodex_core::agent_schedule::AgentWake;
use rusqlite::{Connection, OptionalExtension as _, Transaction, TransactionBehavior};
use serde_json::json;

use crate::{
	EnqueueAgentEvent, SqliteStore, StoreError, agent,
	assistant::{record, records},
	error,
};

const KIND: &str = "agent_internal_wake";

fn invalid() -> StoreError {
	StoreError::InvalidInput("invalid scheduled duty or Agent")
}

fn require_agent(c: &Connection, owner: &str) -> Result<(), StoreError> {
	if c.query_row("SELECT EXISTS(SELECT 1 FROM agents WHERE id=?1)", [owner], |r| {
		r.get::<_, bool>(0)
	})
	.map_err(error::sqlite_error)?
	{
		Ok(())
	} else {
		Err(invalid())
	}
}

fn owners(c: &Connection) -> Result<Vec<String>, StoreError> {
	c.prepare("SELECT DISTINCT e.work_item_id FROM agent_inbox_events e JOIN agents a ON a.id=e.work_item_id WHERE e.event_kind=?1 ORDER BY e.work_item_id")
		.map_err(error::sqlite_error)?.query_map([KIND], |r|r.get(0)).map_err(error::sqlite_error)?
		.collect::<Result<Vec<_>,_>>().map_err(|e|error::sqlite_error(e).into())
}

impl SqliteStore {
	/// Save a direct user-selected duty without selecting or changing a personal Assistant.
	pub async fn schedule_agent_wake(
		&self,
		owner: String,
		key: String,
		wake: AgentWake,
	) -> Result<(), StoreError> {
		self.run(move |c| {
			let tx = c
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(error::sqlite_error)?;
			require_agent(&tx, &owner)?;
			let receipt = json!([KIND, "schedule", owner, key]).to_string();
			let payload = json!({"id":wake.id,"value":wake});
			let encoded_payload = payload.to_string();
			let previous = tx
				.query_row(
					"SELECT payload FROM agent_inbox_events WHERE source_event_id=?1",
					[&receipt],
					|r| r.get::<_, String>(0),
				)
				.optional()
				.map_err(error::sqlite_error)?;
			if let Some(previous) = previous {
				return if previous == encoded_payload {
					Ok(())
				} else {
					Err(StoreError::IdempotencyConflict)
				};
			}
			if wake.id.trim().is_empty()
				|| wake.id.len() > 128
				|| wake.instruction.trim().is_empty()
				|| wake.instruction.len() > 8_192
				|| !wake
					.due_at_micros
					.is_some_and(|due| due > crate::unix_micros().unwrap_or(i64::MAX))
				|| wake.interval_seconds.is_some_and(|s| s < 60)
			{
				return Err(invalid());
			}
			record(&tx, &owner, &receipt, KIND, payload)?;
			tx.commit().map_err(error::sqlite_error)?;
			Ok(())
		})
		.await
	}

	/// Cancel one duty. An admitted native turn is not interrupted.
	pub async fn cancel_agent_wake(
		&self,
		owner: String,
		key: String,
		wake_id: String,
	) -> Result<(), StoreError> {
		self.run(move |c| {
			let tx = c
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(error::sqlite_error)?;
			require_agent(&tx, &owner)?;
			record(
				&tx,
				&owner,
				&json!([KIND, "cancel", owner, key]).to_string(),
				KIND,
				json!({"id":wake_id,"value":null}),
			)?;
			tx.commit().map_err(error::sqlite_error)?;
			Ok(())
		})
		.await
	}
}

pub(crate) fn wakes(c: &Connection, owner: &str) -> Result<Vec<AgentWake>, StoreError> {
	records(c, owner, KIND)
}

pub(crate) fn enqueue_due(tx: &Transaction<'_>, now: i64) -> Result<(), StoreError> {
	for owner in owners(tx)? {
		for mut wake in records::<AgentWake>(tx, &owner, KIND)? {
			let Some(due) = wake.due_at_micros.filter(|due| *due <= now) else {
				continue;
			};
			let key = json!(["agent_wake_due", owner, wake.id, due]).to_string();
			agent::insert_event(tx,EnqueueAgentEvent {source_event_id:key.clone(),work_item_id:owner.clone(),event_kind:"followup_due".into(),payload:json!({"wakeId":wake.id,"instruction":wake.instruction,"authorityEventId":wake.authority_event_id,"dueAtMicros":due,"notificationPolicy":"Notify only on a meaningful change, result, failure or required user action."}).to_string()},false)?;
			wake.due_at_micros = wake.interval_seconds.and_then(|s| {
				let period = i64::from(s) * 1_000_000;
				// Preserve the selected UTC cadence while coalescing missed occurrences.
				due.checked_add(((now - due) / period).checked_add(1)?.checked_mul(period)?)
			});
			record(
				tx,
				&owner,
				&format!("{key}:advance"),
				KIND,
				json!({"id":wake.id,"value":wake}),
			)?;
		}
	}
	Ok(())
}

pub(crate) fn cancel_on_archive(
	tx: &Transaction<'_>,
	owner: &str,
	thread: &str,
) -> Result<(), StoreError> {
	for wake in records::<AgentWake>(tx, owner, KIND)? {
		record(
			tx,
			owner,
			&json!([KIND, "archive", owner, thread, wake.id, wake.due_at_micros]).to_string(),
			KIND,
			json!({"id":wake.id,"value":null}),
		)?;
	}
	Ok(())
}
